//! Debug formatting utilities for macro hygiene debugging
//!
//! This module provides scope-aware formatting of TaggedValues to help debug macro
//! expansion and hygiene issues. When enabled, identifiers are displayed with
//! their scope sets, making it possible to trace how hygiene is being applied.
//!
//! ## Example Output
//!
//! Normal display: `(let ((temp x)) (if temp temp (my-or y)))`
//!
//! Debug display:  `(let{S3} ((temp{S3} x{S1,S2})) (if{S3} temp{S3} temp{S3} (my-or y{S1,S2})))`
//!
//! This makes it clear which identifiers came from the macro template (have S3)
//! versus which came from the use site (have S1,S2 but not S3).

use crate::heap::{Heap, HeapObjectData};
use crate::tagged_value::TaggedValue;
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt::Write;

/// Render a name with its invisible characters spelled out.
///
/// A leading BOM, a zero-width space, a soft hyphen, a bidi control or a C1
/// control all lex as part of an identifier — chibi's any-character-above-
/// ASCII rule, which Patina follows — so `unbound variable: ` could name a
/// variable that renders as nothing at all, or as something that reads
/// exactly like a name that *is* bound. Diagnostics say `\u{200b}` for those
/// rather than printing them (audit F5).
///
/// Everything a reader can actually see is left alone, so the common case
/// allocates nothing.
pub fn escape_invisible(name: &str) -> std::borrow::Cow<'_, str> {
    fn is_invisible(c: char) -> bool {
        matches!(c,
            '\u{00ad}'                  // soft hyphen
            | '\u{200b}'..='\u{200f}'   // zero-width space … RLM
            | '\u{2028}'..='\u{202e}'   // line/para separators, bidi embedding
            | '\u{2060}'..='\u{2064}'   // word joiner, invisible operators
            | '\u{2066}'..='\u{2069}'   // bidi isolates
            | '\u{feff}'                // BOM / zero-width no-break space
            | '\u{0080}'..='\u{009f}'   // C1 controls
        ) || c.is_control()
    }
    if !name.chars().any(is_invisible) {
        return std::borrow::Cow::Borrowed(name);
    }
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if is_invisible(c) {
            let _ = write!(out, "\\u{{{:04x}}}", c as u32);
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Format a TaggedValue for display (without scope annotations)
///
/// Identifiers are shown as plain names without scope sets. A circular value
/// is written with datum labels, as `write` writes it: `#0=(1 2 . #0#)`.
pub fn format_tagged(tv: TaggedValue, heap: &Heap) -> String {
    let mut buf = String::new();
    format_tagged_impl(tv, heap, &mut buf, &mut Printer::new(tv, heap, false));
    buf
}

/// Format a TaggedValue with full scope information for debugging
///
/// Identifiers are annotated with their scope sets for hygiene debugging.
pub fn format_tagged_with_scopes(tv: TaggedValue, heap: &Heap) -> String {
    let mut buf = String::new();
    format_tagged_impl(tv, heap, &mut buf, &mut Printer::new(tv, heap, true));
    buf
}

/// What one call of the formatter carries down: whether to show scopes, and
/// the datum labels of a circular value.
///
/// **Why labels (#457).** This formatter is not only for debugging: the REPL
/// echoes every result through it, `syntax-error` prints its irritants with
/// it, and primitives name a bad argument with it. It had no cycle check, so
/// each of those never returned on a circular value — the REPL stopped
/// reading input, and `(bitwise-and xs 1)` hung inside the `guard` that
/// should have caught its type error. A node reached again from inside
/// itself is labelled as `write` labels it (R7RS 2.4); acyclic output is
/// unchanged, shared structure included, which is printed in full each time
/// as `write` prints it.
struct Printer {
    with_scopes: bool,
    /// The nodes a cycle returns to, by their bits.
    cyclic: FxHashSet<u64>,
    /// The label each has been given, in the order they were first printed.
    labels: FxHashMap<u64, usize>,
}

impl Printer {
    fn new(tv: TaggedValue, heap: &Heap, with_scopes: bool) -> Self {
        let mut budget = CycleSearch::BUDGET;
        let cyclic = if CycleSearch::finishes_within(tv, heap, &mut budget) {
            FxHashSet::default()
        } else {
            let mut search = CycleSearch::default();
            search.visit(tv, heap);
            search.cyclic
        };
        Printer {
            with_scopes,
            cyclic,
            labels: FxHashMap::default(),
        }
    }

    /// Write `#n#` and answer `true` if `tv` has been labelled already;
    /// otherwise, if a cycle returns to it, write `#n=` before it is printed.
    fn label(&mut self, tv: TaggedValue, buf: &mut String) -> bool {
        // Nearly every value has no cycle, and then this is every node's cost.
        if self.cyclic.is_empty() {
            return false;
        }
        let key = tv.raw_bits();
        if !self.cyclic.contains(&key) {
            return false;
        }
        if let Some(n) = self.labels.get(&key) {
            write!(buf, "#{n}#").unwrap();
            return true;
        }
        let n = self.labels.len();
        self.labels.insert(key, n);
        write!(buf, "#{n}=").unwrap();
        false
    }

    fn is_cyclic(&self, tv: TaggedValue) -> bool {
        !self.cyclic.is_empty() && self.cyclic.contains(&tv.raw_bits())
    }
}

/// A depth-first search for the nodes a cycle returns to: a node met again
/// while it is still on the path from the root.
///
/// A list's spine is walked in a loop, not by recursing on each cdr, so a
/// long list costs no stack here — the same shape the printer has. Its pairs
/// stay on the path until the whole list is done, since an element can lead
/// back to any of them.
#[derive(Default)]
struct CycleSearch {
    on_path: FxHashSet<u64>,
    done: FxHashSet<u64>,
    cyclic: FxHashSet<u64>,
}

impl CycleSearch {
    /// How many compound nodes the cheap walk visits before giving up.
    const BUDGET: usize = 1024;

    /// Whether a plain walk of `tv` — every node, as the printer visits them
    /// — ends before `budget` compound nodes. If it does, `tv` has no cycle,
    /// since a walk into a cycle never ends; if not, `tv` is circular or
    /// merely large, and [`CycleSearch::visit`] tells which.
    ///
    /// The search allocates two sets and hashes every node, and a value
    /// printed here is nearly always small and acyclic. The macro matcher
    /// formats its input on every failed attempt at a rule, so paying for
    /// the search on each call made `cond`- and `case`-heavy desugaring 15%
    /// slower. This walk allocates nothing. Measured 2026-09-23, best of 9
    /// interleaved: 1% on a loop that only desugars such forms, and nothing
    /// on chibi's R7RS suite.
    fn finishes_within(tv: TaggedValue, heap: &Heap, budget: &mut usize) -> bool {
        let mut current = tv;
        while Self::has_children(current, heap) {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            if current.is_pair() {
                if !Self::finishes_within(heap.car(current), heap, budget) {
                    return false;
                }
                current = heap.cdr(current);
                continue;
            }
            if current.is_vector() {
                for i in 0..heap.vector_len(current) {
                    if !Self::finishes_within(heap.vector_ref(current, i), heap, budget) {
                        return false;
                    }
                }
            } else {
                match heap.get_object(current) {
                    HeapObjectData::Values(vals) => {
                        for val in vals.iter() {
                            if !Self::finishes_within(*val, heap, budget) {
                                return false;
                            }
                        }
                    }
                    HeapObjectData::MutableCell(cell) => {
                        let content = *cell.borrow();
                        if !Self::finishes_within(content, heap, budget) {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            return true;
        }
        true
    }

    /// Whether a cycle can pass through `tv`: the nodes the printer recurses
    /// into.
    fn has_children(tv: TaggedValue, heap: &Heap) -> bool {
        tv.is_pair()
            || tv.is_vector()
            || (tv.is_object()
                && matches!(
                    heap.get_object(tv),
                    HeapObjectData::Values(_) | HeapObjectData::MutableCell(_)
                ))
    }

    /// `false` when `tv` needs no visit: seen before, or closing a cycle —
    /// which is recorded.
    fn enter(&mut self, tv: TaggedValue) -> bool {
        let key = tv.raw_bits();
        if self.on_path.contains(&key) {
            self.cyclic.insert(key);
            return false;
        }
        if self.done.contains(&key) {
            return false;
        }
        self.on_path.insert(key);
        true
    }

    fn leave(&mut self, tv: TaggedValue) {
        let key = tv.raw_bits();
        self.on_path.remove(&key);
        self.done.insert(key);
    }

    fn visit(&mut self, tv: TaggedValue, heap: &Heap) {
        if !Self::has_children(tv, heap) {
            return;
        }
        if tv.is_pair() {
            let mut spine = Vec::new();
            let mut current = tv;
            while current.is_pair() && self.enter(current) {
                spine.push(current);
                self.visit(heap.car(current), heap);
                current = heap.cdr(current);
            }
            if !current.is_pair() {
                self.visit(current, heap);
            }
            for pair in spine {
                self.leave(pair);
            }
            return;
        }
        if !self.enter(tv) {
            return;
        }
        if tv.is_vector() {
            for i in 0..heap.vector_len(tv) {
                self.visit(heap.vector_ref(tv, i), heap);
            }
        } else {
            match heap.get_object(tv) {
                HeapObjectData::Values(vals) => {
                    for val in vals.iter() {
                        self.visit(*val, heap);
                    }
                }
                HeapObjectData::MutableCell(cell) => {
                    let content = *cell.borrow();
                    self.visit(content, heap);
                }
                _ => {}
            }
        }
        self.leave(tv);
    }
}

/// Format a real (f64) value in Scheme display style
///
/// Handles +inf.0, -inf.0, +nan.0, and ensures all inexact numbers
/// have a decimal point (e.g., "1.0" not "1").
///
/// Note: This is for display/write output. `number->string` uses a separate
/// implementation in conversion.rs that also handles -0.0 and scientific notation.
pub fn format_real(r: f64, buf: &mut String) {
    if r.is_infinite() {
        if r.is_sign_positive() {
            buf.push_str("+inf.0");
        } else {
            buf.push_str("-inf.0");
        }
    } else if r.is_nan() {
        buf.push_str("+nan.0");
    } else if r.fract() == 0.0 {
        write!(buf, "{:.1}", r).unwrap();
    } else {
        write!(buf, "{}", r).unwrap();
    }
}

/// Format a complex number from its real and imaginary TaggedValue parts
fn format_complex(real: TaggedValue, imag: TaggedValue, heap: &Heap, buf: &mut String) {
    // Format parts into temporary strings so we can inspect them
    let real_str = format_tagged(real, heap);
    let imag_str = format_tagged(imag, heap);

    // Asked of the values, not of their spelling: `"0.0"` is a zero but an
    // *inexact* one, and dropping it writes a different number — `+2.0i` and
    // `1.0` read back with exact zero parts. This is the formatter the REPL
    // prints results with, so it has to agree with `write` and
    // `number->string`, which ask `Heap::is_exact_zero` too.
    let real_is_zero = heap.is_exact_zero(real);
    let imag_is_zero = heap.is_exact_zero(imag);

    if real_is_zero && imag_is_zero {
        buf.push('0');
    } else if real_is_zero {
        // Pure imaginary
        if imag_str == "1" {
            buf.push_str("+i");
        } else if imag_str == "-1" {
            buf.push_str("-i");
        } else if imag_str.starts_with('-') || imag_str.starts_with('+') {
            write!(buf, "{}i", imag_str).unwrap();
        } else {
            write!(buf, "+{}i", imag_str).unwrap();
        }
    } else if imag_is_zero {
        buf.push_str(&real_str);
    } else if imag_str == "1" {
        write!(buf, "{}+i", real_str).unwrap();
    } else if imag_str == "-1" {
        write!(buf, "{}-i", real_str).unwrap();
    } else if imag_str.starts_with('-') || imag_str.starts_with('+') {
        write!(buf, "{}{}i", real_str, imag_str).unwrap();
    } else {
        write!(buf, "{}+{}i", real_str, imag_str).unwrap();
    }
}

/// Format a HeapObjectData value
fn format_object(obj: &HeapObjectData, heap: &Heap, buf: &mut String, printer: &mut Printer) {
    match obj {
        HeapObjectData::BigInt(n) => write!(buf, "{}", n).unwrap(),
        HeapObjectData::Rational(r) => write!(buf, "{}", r).unwrap(),
        HeapObjectData::Real(r) => format_real(*r, buf),
        HeapObjectData::Complex { real, imag } => format_complex(*real, *imag, heap, buf),
        HeapObjectData::Symbol(s) => buf.push_str(s),
        HeapObjectData::Ephemeron(_) => buf.push_str("#<ephemeron>"),
        HeapObjectData::Identifier { name, scopes } => {
            buf.push_str(name);
            if printer.with_scopes && !scopes.is_empty() {
                write!(buf, "{}", scopes).unwrap();
            }
        }
        // Space-separated, not Rust's `{:?}` — that printed `#u8([1, 2])`,
        // which is not Scheme, in the one place this formatter is
        // user-facing: the REPL echoes results through `format_tagged`.
        HeapObjectData::Bytevector(bv) => {
            buf.push_str("#u8(");
            for (i, b) in bv.iter().enumerate() {
                if i > 0 {
                    buf.push(' ');
                }
                write!(buf, "{}", b).unwrap();
            }
            buf.push(')');
        }
        // The message alone, where `display` and `write` also print the
        // irritants. Chosen when this formatter had no cycle detection, so
        // that `(error "cycle" xs)` with a circular `xs` could not hang it;
        // it has labels now (#457), and the choice stands on its own — an
        // irritant list can be long, and this is a one-line summary.
        HeapObjectData::Exception { message, .. } => {
            write!(buf, "#<error-object: {}>", message).unwrap()
        }
        HeapObjectData::Procedure(p) => {
            use crate::procedure::Procedure;
            match p.as_ref() {
                Procedure::Primitive { qualified_name, .. } => {
                    // qualified_name is "library/name", display as "library:name"
                    if let Some(pos) = qualified_name.find('/') {
                        write!(
                            buf,
                            "#<procedure:{}:{}>",
                            &qualified_name[..pos],
                            &qualified_name[pos + 1..]
                        )
                        .unwrap()
                    } else {
                        write!(buf, "#<procedure:{}>", qualified_name).unwrap()
                    }
                }
                Procedure::CpsLambda { .. } => buf.push_str("#<procedure>"),
            }
        }
        HeapObjectData::Port(p) => write!(buf, "{}", p).unwrap(),
        HeapObjectData::Macro(m) => write!(buf, "#<macro:{}>", m.name).unwrap(),
        // Reachable from macro-debug output and from the residual cases in
        // `patina-tests/tests/syntax_as_a_value.rs` — a marker no longer
        // survives an ordinary variable reference. Prints its canonical
        // spelling, not the name it was reached by, so a `begin` imported as
        // `blk` is still visibly `begin`.
        HeapObjectData::CoreSyntax(form) => write!(buf, "#<syntax:{}>", form).unwrap(),
        HeapObjectData::RecordType(rtd) => write!(buf, "#<record-type {}>", rtd.name).unwrap(),
        HeapObjectData::Record { record_type, .. } => {
            write!(buf, "#<record {}>", record_type.name).unwrap()
        }
        HeapObjectData::Continuation(_) => buf.push_str("#<continuation>"),
        HeapObjectData::Parameter { .. } => buf.push_str("#<parameter>"),
        HeapObjectData::Promise(_) => buf.push_str("#<promise>"),
        HeapObjectData::Library(lib) => write!(buf, "{}", lib).unwrap(),
        HeapObjectData::Values(vals) => {
            for (i, val) in vals.iter().enumerate() {
                if i > 0 {
                    buf.push('\n');
                }
                format_tagged_impl(*val, heap, buf, printer);
            }
        }
        HeapObjectData::EnvironmentSpecifier { .. } => buf.push_str("#<environment>"),
        HeapObjectData::PromptTag(tag) => write!(buf, "{}", tag).unwrap(),
        HeapObjectData::LabelPlaceholder(n) => write!(buf, "#<label-placeholder:{}>", n).unwrap(),
        HeapObjectData::VmClosure { code_id, .. } => {
            // The VM's `CodeObjectId`: its slot in the low half and its
            // generation in the high, printed as the VM's traces print it.
            let (slot, generation) = (*code_id as u32, (*code_id >> 32) as u32);
            match generation {
                0 => write!(buf, "#<procedure:{slot}>").unwrap(),
                _ => write!(buf, "#<procedure:{slot}.{generation}>").unwrap(),
            }
        }
        HeapObjectData::MutableCell(cell) => {
            buf.push_str("#<cell:");
            let content = *cell.borrow();
            format_tagged_impl(content, heap, buf, printer);
            buf.push('>');
        }
        HeapObjectData::VmContinuationRef(id) => write!(buf, "#<continuation:{}>", id).unwrap(),
        HeapObjectData::VmDelimitedContinuationRef(id) => {
            write!(buf, "#<delimited-continuation:{}>", id).unwrap()
        }
        HeapObjectData::Free => buf.push_str("#<gc-freed-slot>"),
    }
}

/// Unified recursive formatter for TaggedValue
fn format_tagged_impl(tv: TaggedValue, heap: &Heap, buf: &mut String, printer: &mut Printer) {
    if printer.label(tv, buf) {
        return;
    }

    // Immediate values
    if tv.is_fixnum() {
        write!(buf, "{}", tv.as_fixnum_unchecked()).unwrap();
        return;
    }
    if tv == TaggedValue::TRUE {
        buf.push_str("#t");
        return;
    }
    if tv == TaggedValue::FALSE {
        buf.push_str("#f");
        return;
    }
    if tv == TaggedValue::NULL {
        buf.push_str("()");
        return;
    }
    if tv.is_char() {
        let c = tv.as_char_unchecked();
        write!(buf, "#\\{}", c).unwrap();
        return;
    }
    if tv == TaggedValue::EOF {
        buf.push_str("#<eof>");
        return;
    }
    if tv == TaggedValue::UNSPECIFIED {
        buf.push_str("#<unspecified>");
        return;
    }

    // Native pairs
    if tv.is_pair() {
        buf.push('(');
        format_tagged_list(tv, heap, buf, printer);
        buf.push(')');
        return;
    }

    // Native strings
    if tv.is_string() {
        let s = heap.get_string_as_utf8(tv);
        write!(buf, "\"{}\"", s).unwrap();
        return;
    }

    // Native vectors
    if tv.is_vector() {
        buf.push_str("#(");
        let len = heap.vector_len(tv);
        for i in 0..len {
            if i > 0 {
                buf.push(' ');
            }
            format_tagged_impl(heap.vector_ref(tv, i), heap, buf, printer);
        }
        buf.push(')');
        return;
    }

    // Object types
    if tv.is_object() {
        let obj = heap.get_object(tv);
        format_object(obj, heap, buf, printer);
        return;
    }

    // Closures or other unknown tags
    buf.push_str("#<object>");
}

/// Format tagged list contents, handling dotted lists
///
/// A pair in the spine that a cycle returns to is written as a dotted tail,
/// so that its label has somewhere to go: `(1 2 . #0#)`.
fn format_tagged_list(tv: TaggedValue, heap: &Heap, buf: &mut String, printer: &mut Printer) {
    let mut current = tv;
    let mut first = true;

    loop {
        if current == TaggedValue::NULL {
            break;
        }
        if current.is_pair() && (first || !printer.is_cyclic(current)) {
            let car = heap.car(current);
            let cdr = heap.cdr(current);
            if !first {
                buf.push(' ');
            }
            first = false;
            format_tagged_impl(car, heap, buf, printer);
            current = cdr;
        } else {
            // Dotted list tail
            buf.push_str(" . ");
            format_tagged_impl(current, heap, buf, printer);
            break;
        }
    }
}

#[cfg(test)]
mod complex_spelling_tests {
    use super::format_tagged;
    use crate::heap::Heap;

    /// The REPL prints results through `format_tagged`, so it has to spell a
    /// complex number the way `write` and `number->string` do.
    ///
    /// A zero part may be dropped only when it is *exact*: `+2.0i` and `1.0`
    /// read back with exact zero parts, so using them for
    /// `(make-rectangular 0.0 2.0)` or `1.0+0.0i` names a different number.
    /// This formatter decided by comparing the formatted string against "0.0",
    /// which cannot see the difference, and so kept printing `+2.0i` after the
    /// other two writers had stopped.
    #[test]
    fn a_zero_part_is_dropped_only_when_exact() {
        let mut heap = Heap::new();
        let cases = [
            (0.0_f64, 2.0_f64, "0.0+2.0i"),
            (1.0, 0.0, "1.0+0.0i"),
            (1.0, 2.0, "1.0+2.0i"),
            (0.0, -2.0, "0.0-2.0i"),
        ];
        for (re, im, expected) in cases {
            let r = heap.alloc_real(re);
            let i = heap.alloc_real(im);
            let z = heap.alloc_complex(r, i);
            assert_eq!(format_tagged(z, &heap), expected, "for {re}+{im}i");
        }

        // Exact zeros still drop, which is what makes the short forms mean
        // what they say.
        let zero = crate::TaggedValue::fixnum(0);
        let two = heap.alloc_real(2.0);
        let z = heap.alloc_complex(zero, two);
        assert_eq!(format_tagged(z, &heap), "+2.0i");
    }
}

/// Datum labels for circular values (#457). Each expected string is what
/// chibi 0.12, Gauche 0.9.15 and Patina's own `write` print for the same
/// value, measured 2026-09-23.
#[cfg(test)]
mod cycle_tests {
    use super::format_tagged;
    use crate::TaggedValue;
    use crate::heap::Heap;

    fn fx(n: i64) -> TaggedValue {
        TaggedValue::fixnum(n)
    }

    /// `(1 2)` with its last cdr pointed back at its head.
    fn circular_list(heap: &mut Heap) -> TaggedValue {
        let xs = heap.list_from_iter([fx(1), fx(2)]);
        let last = heap.cdr(xs);
        heap.set_cdr(last, xs);
        xs
    }

    #[test]
    fn a_circular_list_is_labelled() {
        let mut heap = Heap::new();
        let xs = circular_list(&mut heap);
        assert_eq!(format_tagged(xs, &heap), "#0=(1 2 . #0#)");
    }

    #[test]
    fn a_second_reference_to_a_labelled_node_is_its_label() {
        let mut heap = Heap::new();
        let xs = circular_list(&mut heap);
        let v = heap.alloc_vector(vec![xs, xs]);
        assert_eq!(format_tagged(v, &heap), "#(#0=(1 2 . #0#) #0#)");
    }

    #[test]
    fn a_cycle_into_the_middle_of_a_list_breaks_the_spine_there() {
        let mut heap = Heap::new();
        let xs = circular_list(&mut heap);
        let a = heap.intern_symbol("a");
        let outer = heap.alloc_pair(a, xs);
        assert_eq!(format_tagged(outer, &heap), "(a . #0=(1 2 . #0#))");
        let wrapped = heap.list_from_iter([a, xs]);
        assert_eq!(format_tagged(wrapped, &heap), "(a #0=(1 2 . #0#))");
    }

    #[test]
    fn cycles_through_a_car_and_through_a_vector() {
        let mut heap = Heap::new();
        let q = heap.intern_symbol("q");
        let z = heap.alloc_pair(q, TaggedValue::NULL);
        heap.set_car(z, z);
        assert_eq!(format_tagged(z, &heap), "#0=(#0#)");

        let v = heap.alloc_vector(vec![fx(1), fx(2)]);
        heap.vector_set(v, 1, v);
        let sym = heap.intern_symbol("z");
        let l = heap.list_from_iter([v, sym]);
        assert_eq!(format_tagged(l, &heap), "(#0=#(1 #0#) z)");
    }

    /// Only a cycle is labelled, as `write` labels (R7RS 6.13.3); structure
    /// that is merely shared prints in full each time.
    #[test]
    fn shared_structure_that_is_not_circular_is_printed_in_full() {
        let mut heap = Heap::new();
        let ys = heap.list_from_iter([fx(1), fx(2)]);
        let both = heap.list_from_iter([ys, ys]);
        assert_eq!(format_tagged(both, &heap), "((1 2) (1 2))");
    }

    /// The search walks a spine in a loop, as the printer does, so a long
    /// list costs it no stack.
    #[test]
    fn a_long_list_is_searched_without_recursing_on_its_spine() {
        let mut heap = Heap::new();
        let xs = heap.list_from_iter((0..200_000).map(fx));
        let printed = format_tagged(xs, &heap);
        assert!(printed.starts_with("(0 1 2 "), "{}", &printed[..20]);
        assert!(printed.ends_with(" 199999)"));
    }
}
