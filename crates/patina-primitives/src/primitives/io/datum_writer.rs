//! Datum Label Writer for handling circular and shared structures
//!
//! This module implements R7RS datum labels (#n= and #n#) for handling
//! circular and shared structures in output operations.
//!
//! The write path uses TaggedValue directly (write_tagged, find_circular_tagged)
//! for all compound types (pairs, vectors, strings).
//! Leaf object types (BigInt, Symbol, etc.) are formatted directly via Heap methods.

use patina_core::debug_format::format_real;
use patina_core::{Heap, TaggedValue};
use std::cell::RefCell;
use std::collections::HashMap;

/// Format a Real (f64) number in R7RS style (delegates to patina_core::debug_format::format_real)
fn format_real_number(f: f64, out: &mut String) {
    format_real(f, out);
}

/// Format a complex number from its real and imaginary TaggedValue parts.
///
/// R7RS rules:
/// - Zero real part is omitted: `0+5i` → `+5i`
/// - Exact ±1 imaginary is displayed as `±i`: `3+1i` → `3+i`
/// - Inexact ±1.0 imaginary keeps the number: `3.0+1.0i` stays `3.0+1.0i`
fn format_complex(real_tv: TaggedValue, imag_tv: TaggedValue, heap: &Heap, out: &mut String) {
    // Helper: check if TaggedValue is exact zero
    let is_zero = |tv: TaggedValue| -> bool {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == 0;
        }
        if let Some(f) = heap.get_real(tv) {
            return f == 0.0;
        }
        if let Some(n) = heap.get_bigint(tv) {
            return n.sign() == num_bigint::Sign::NoSign;
        }
        if let Some(r) = heap.get_rational(tv) {
            use num_traits::Zero;
            return r.is_zero();
        }
        false
    };

    // Helper: check if TaggedValue is exact 1 (not inexact 1.0)
    let is_exact_one = |tv: TaggedValue| -> bool {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == 1;
        }
        if let Some(n) = heap.get_bigint(tv) {
            use num_traits::One;
            return n == &num_bigint::BigInt::one();
        }
        if let Some(r) = heap.get_rational(tv) {
            use num_traits::One;
            return r.is_one();
        }
        false
    };

    // Helper: check if TaggedValue is exact -1 (not inexact -1.0)
    let is_exact_neg_one = |tv: TaggedValue| -> bool {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == -1;
        }
        if let Some(n) = heap.get_bigint(tv) {
            use num_traits::One;
            return n == &(-num_bigint::BigInt::one());
        }
        if let Some(r) = heap.get_rational(tv) {
            use num_traits::One;
            return r == &(-num_rational::BigRational::one());
        }
        false
    };

    let real_is_zero = is_zero(real_tv);
    let imag_is_zero = is_zero(imag_tv);

    if real_is_zero && imag_is_zero {
        out.push('0');
        return;
    }

    if imag_is_zero {
        // Pure real
        format_tagged_leaf(real_tv, heap, out);
        return;
    }

    if real_is_zero {
        // Pure imaginary
        if is_exact_one(imag_tv) {
            out.push_str("+i");
        } else if is_exact_neg_one(imag_tv) {
            out.push_str("-i");
        } else {
            let mut imag_str = String::new();
            format_tagged_leaf(imag_tv, heap, &mut imag_str);
            if imag_str.starts_with('+') || imag_str.starts_with('-') {
                out.push_str(&imag_str);
            } else {
                out.push('+');
                out.push_str(&imag_str);
            }
            out.push('i');
        }
        return;
    }

    // Both parts non-zero
    format_tagged_leaf(real_tv, heap, out);

    if is_exact_one(imag_tv) {
        out.push_str("+i");
    } else if is_exact_neg_one(imag_tv) {
        out.push_str("-i");
    } else {
        let mut imag_str = String::new();
        format_tagged_leaf(imag_tv, heap, &mut imag_str);
        if imag_str.starts_with('+') || imag_str.starts_with('-') {
            out.push_str(&imag_str);
        } else {
            out.push('+');
            out.push_str(&imag_str);
        }
        out.push('i');
    }
}

/// Check if a symbol name needs vertical bar notation when written
///
/// Returns true if the symbol contains characters that require vertical bars
/// to be read back correctly (spaces, parentheses, starts with digit, etc.)
fn symbol_needs_vertical_bars(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }

    let first_char = name.chars().next().unwrap();

    // Starts with digit -> looks like number
    if first_char.is_ascii_digit() {
        return true;
    }

    // Check for +/- prefix (complex number or signed number)
    if (first_char == '+' || first_char == '-') && name.len() > 1 {
        let second_char = name.chars().nth(1).unwrap();

        if second_char.is_ascii_digit() {
            return true;
        }

        // +/-.digit -> number (e.g., -.4)
        if second_char == '.' && name.len() > 2 {
            let third_char = name.chars().nth(2).unwrap();
            if third_char.is_ascii_digit() {
                return true;
            }
        }

        // +i, -i, +I, -I -> imaginary number
        if matches!(second_char, 'i' | 'I') && name.len() == 2 {
            return true;
        }

        // Check for special floats or things that start with them (case insensitive)
        let lower = name.to_lowercase();
        if lower == "+inf.0"
            || lower == "-inf.0"
            || lower == "+nan.0"
            || lower == "-nan.0"
            || lower.starts_with("+inf.0")
            || lower.starts_with("-inf.0")
            || lower.starts_with("+nan.0")
            || lower.starts_with("-nan.0")
        {
            return true;
        }
    }

    // Single dot needs bars
    if name == "." {
        return true;
    }

    for (i, ch) in name.chars().enumerate() {
        if !ch.is_ascii() {
            return true;
        }

        if i == 0 {
            // Intentionally *stricter* than the lexer's `is_identifier_start`,
            // which also admits `@` and non-ASCII letters. This asks "would
            // every R7RS reader take this bare?", so it stays at 7.1.1 and
            // bar-quotes the rest — `@` writes as `|@|`, as Gauche does. Do
            // not sync the two lists; widening what we *read* must not narrow
            // what we escape.
            let is_valid_initial = ch.is_ascii_alphabetic()
                || matches!(
                    ch,
                    '!' | '$'
                        | '%'
                        | '&'
                        | '*'
                        | '/'
                        | ':'
                        | '<'
                        | '='
                        | '>'
                        | '?'
                        | '^'
                        | '_'
                        | '~'
                        | '+'
                        | '-'
                        | '.'
                );
            if !is_valid_initial {
                return true;
            }
        } else {
            let is_valid_subsequent = ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '!' | '$'
                        | '%'
                        | '&'
                        | '*'
                        | '/'
                        | ':'
                        | '<'
                        | '='
                        | '>'
                        | '?'
                        | '^'
                        | '_'
                        | '~'
                        | '+'
                        | '-'
                        | '.'
                        | '@'
                );
            if !is_valid_subsequent {
                return true;
            }
        }
    }

    false
}

/// Escape special characters for display inside vertical bar notation.
/// R7RS requires that `|` and `\` be escaped as `\|` and `\\` respectively.
fn escape_for_vertical_bar(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '|' => result.push_str("\\|"),
            '\\' => result.push_str("\\\\"),
            _ => result.push(ch),
        }
    }
    result
}

/// Write a symbol name, wrapping in vertical bars if needed (write mode only).
fn write_symbol_name(name: &str, display_mode: bool, out: &mut String) {
    if !display_mode && symbol_needs_vertical_bars(name) {
        out.push('|');
        out.push_str(&escape_for_vertical_bar(name));
        out.push('|');
    } else {
        out.push_str(name);
    }
}

/// Format a leaf (non-compound) TAG_OBJECT using Heap methods directly.
/// Handles Symbol, BigInt, Rational, Real, Complex, Bytevector, Record, etc.
fn format_leaf_object(tv: TaggedValue, heap: &Heap, display_mode: bool, out: &mut String) {
    // Closure (TAG_CLOSURE)
    if tv.is_closure() {
        out.push_str("#<procedure>");
        return;
    }
    // Procedure (HeapObjectData::Procedure — CPS lambdas, primitives)
    if heap.get_procedure(tv).is_some() {
        out.push_str("#<procedure>");
        return;
    }
    // Environment specifier
    if heap.is_environment(tv) {
        out.push_str("#<environment>");
        return;
    }
    // Symbol
    if let Some(name) = heap.get_symbol_name(tv) {
        write_symbol_name(name, display_mode, out);
        return;
    }
    // Identifier
    if let Some((name, _)) = heap.get_identifier_data_any(tv) {
        write_symbol_name(&name, display_mode, out);
        return;
    }
    // BigInt
    if let Some(n) = heap.get_bigint(tv) {
        out.push_str(&n.to_string());
        return;
    }
    // Rational
    if let Some(r) = heap.get_rational(tv) {
        out.push_str(&r.to_string());
        return;
    }
    // Real
    if let Some(f) = heap.get_real(tv) {
        format_real_number(f, out);
        return;
    }
    // Complex
    if let Some((real_tv, imag_tv)) = heap.get_complex(tv) {
        format_complex(real_tv, imag_tv, heap, out);
        return;
    }
    // Bytevector
    if let Some(bytes) = heap.get_bytevector(tv) {
        out.push_str("#u8(");
        for (i, b) in bytes.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&b.to_string());
        }
        out.push(')');
        return;
    }
    // Port
    if heap.get_port(tv).is_some() {
        out.push_str("#<port>");
        return;
    }
    // RecordType
    if let Some(rtd) = heap.get_record_type(tv) {
        use std::fmt::Write;
        write!(out, "#<record-type {}>", rtd.name).unwrap();
        return;
    }
    // Record
    if let Some((rtd, _)) = heap.get_record(tv) {
        use std::fmt::Write;
        write!(out, "#<record {}>", rtd.name).unwrap();
        return;
    }
    // Promise
    if heap.get_promise(tv).is_some() {
        out.push_str("#<promise>");
        return;
    }
    // Macro
    if heap.get_macro(tv).is_some() {
        out.push_str("#<macro>");
        return;
    }
    // Values
    if heap.get_values(tv).is_some() {
        out.push_str("#<values>");
        return;
    }
    // Parameter object — a procedure per R7RS §4.2.6, named for what it is.
    // Last in the chain: every probe above it is paid by every leaf formatted.
    if heap.is_parameter(tv) {
        out.push_str("#<parameter>");
        return;
    }
    // Unknown
    out.push_str("#<unknown>");
}

/// Format a TaggedValue as a leaf (non-compound) for numeric display.
/// Handles inline types and delegates to format_leaf_object for TAG_OBJECT.
/// Used internally by format_complex — always uses write mode for numbers.
fn format_tagged_leaf(tv: TaggedValue, heap: &Heap, out: &mut String) {
    if tv.is_fixnum() {
        out.push_str(&tv.as_fixnum_unchecked().to_string());
    } else if tv.is_boolean() {
        out.push_str(if tv.as_bool_unchecked() { "#t" } else { "#f" });
    } else if tv.is_null() {
        out.push_str("()");
    } else if tv.is_char() {
        out.push_str(&tv.to_string());
    } else if tv.is_eof() {
        out.push_str("#<eof>");
    } else if tv == TaggedValue::UNSPECIFIED {
        out.push_str("#<unspecified>");
    } else {
        format_leaf_object(tv, heap, false, out);
    }
}

/// Writer that handles circular/shared structures using datum labels
pub(super) struct DatumLabelWriter<'a> {
    /// Maps pointer addresses to label numbers for circular structures
    labels: HashMap<usize, usize>,
    /// Set of addresses currently on the traversal stack (for detecting cycles)
    on_stack: HashMap<usize, bool>,
    /// Next label number to assign
    next_label: usize,
    /// Whether we're in display mode (strings without quotes, chars without #\)
    display_mode: bool,
    /// Whether to label all shared structures (write-shared) or only circular (write)
    label_shared: bool,
    /// Heap reference for accessing heap-allocated values
    heap: &'a RefCell<Heap>,
}

impl<'a> DatumLabelWriter<'a> {
    pub(super) fn new(display_mode: bool, label_shared: bool, heap: &'a RefCell<Heap>) -> Self {
        Self {
            labels: HashMap::new(),
            on_stack: HashMap::new(),
            next_label: 0,
            display_mode,
            label_shared,
            heap,
        }
    }

    // =========================================================================
    // Pass 1: Find circular (and optionally shared) structures
    // =========================================================================

    /// Find circular structures starting from a TaggedValue (primary path).
    /// Only recurses into compound types (pairs, vectors). Immediate types,
    /// strings, and closures have no sub-structure to traverse.
    pub(super) fn find_circular_tagged(&mut self, tv: TaggedValue) {
        if tv.is_pair() {
            let addr = tv.raw() as usize;

            if self.on_stack.get(&addr).copied().unwrap_or(false) {
                if !self.labels.contains_key(&addr) {
                    self.labels.insert(addr, self.next_label);
                    self.next_label += 1;
                }
                return;
            }

            if self.on_stack.contains_key(&addr) {
                if self.label_shared && !self.labels.contains_key(&addr) {
                    self.labels.insert(addr, self.next_label);
                    self.next_label += 1;
                }
                return;
            }

            self.on_stack.insert(addr, true);
            let (car_tv, cdr_tv) = {
                let heap_ref = self.heap.borrow();
                (heap_ref.car(tv), heap_ref.cdr(tv))
            };
            self.find_circular_tagged(car_tv);
            self.find_circular_tagged(cdr_tv);
            self.on_stack.insert(addr, false);
        } else if tv.is_vector() {
            let addr = tv.raw() as usize;

            if self.on_stack.get(&addr).copied().unwrap_or(false) {
                if !self.labels.contains_key(&addr) {
                    self.labels.insert(addr, self.next_label);
                    self.next_label += 1;
                }
                return;
            }

            if self.on_stack.contains_key(&addr) {
                if self.label_shared && !self.labels.contains_key(&addr) {
                    self.labels.insert(addr, self.next_label);
                    self.next_label += 1;
                }
                return;
            }

            self.on_stack.insert(addr, true);
            let elements: Vec<TaggedValue> = {
                let heap_ref = self.heap.borrow();
                heap_ref.vector_slice(tv).to_vec()
            };
            for elem_tv in &elements {
                self.find_circular_tagged(*elem_tv);
            }
            self.on_stack.insert(addr, false);
        }
        // Immediate types, strings, closures, other objects: no compound structure to traverse
    }

    // =========================================================================
    // Pass 2: Write with labels
    // =========================================================================

    /// Write a TaggedValue with datum labels (primary path).
    /// Handles immediate types and native heap types directly.
    pub(super) fn write_tagged(
        &self,
        tv: TaggedValue,
        out: &mut String,
        emitted: &mut HashMap<usize, bool>,
    ) {
        // Immediate types — format directly
        if tv.is_fixnum() {
            out.push_str(&tv.as_fixnum_unchecked().to_string());
            return;
        }
        if tv.is_null() {
            out.push_str("()");
            return;
        }
        if tv.is_boolean() {
            out.push_str(if tv.as_bool_unchecked() { "#t" } else { "#f" });
            return;
        }
        if tv.is_char() {
            let c = tv.as_char_unchecked();
            if self.display_mode {
                out.push(c);
            } else {
                // TaggedValue::Display handles named chars (#\space, #\newline, etc.)
                out.push_str(&tv.to_string());
            }
            return;
        }
        if tv.is_eof() {
            out.push_str("#<eof>");
            return;
        }
        if tv == TaggedValue::UNSPECIFIED {
            out.push_str("#<unspecified>");
            return;
        }

        // Native heap pair
        if tv.is_pair() {
            let addr = tv.raw() as usize;

            if let Some(&label) = self.labels.get(&addr) {
                if emitted.get(&addr).copied().unwrap_or(false) {
                    out.push_str(&format!("#{}#", label));
                    return;
                } else {
                    emitted.insert(addr, true);
                    out.push_str(&format!("#{}=", label));
                }
            }

            if let Some(shorthand) = self.check_quote_shorthand_tagged(tv) {
                out.push_str(&shorthand);
                return;
            }

            out.push('(');
            self.write_tagged_list_contents(tv, out, emitted);
            out.push(')');
            return;
        }

        // Native heap vector
        if tv.is_vector() {
            let addr = tv.raw() as usize;

            if let Some(&label) = self.labels.get(&addr) {
                if emitted.get(&addr).copied().unwrap_or(false) {
                    out.push_str(&format!("#{}#", label));
                    return;
                } else {
                    emitted.insert(addr, true);
                    out.push_str(&format!("#{}=", label));
                }
            }

            let elements: Vec<TaggedValue> = {
                let heap_ref = self.heap.borrow();
                heap_ref.vector_slice(tv).to_vec()
            };
            out.push_str("#(");
            for (i, elem_tv) in elements.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                self.write_tagged(*elem_tv, out, emitted);
            }
            out.push(')');
            return;
        }

        // Native heap string
        if tv.is_string() {
            let heap_ref = self.heap.borrow();
            let chars = heap_ref.get_string_chars(tv);
            if self.display_mode {
                for &c in chars {
                    out.push(c);
                }
            } else {
                out.push('"');
                for &c in chars {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if c.is_control() => {
                            out.push_str(&format!("\\x{:02x};", c as u32));
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            return;
        }

        // HeapObjectData leaf types — format using heap methods
        let heap_ref = self.heap.borrow();
        format_leaf_object(tv, &heap_ref, self.display_mode, out);
    }

    /// Write list contents from a native heap pair (TaggedValue).
    /// Gets car/cdr as TaggedValues and formats without conversion.
    fn write_tagged_list_contents(
        &self,
        tv: TaggedValue,
        out: &mut String,
        emitted: &mut HashMap<usize, bool>,
    ) {
        let (car_tv, cdr_tv) = {
            let heap_ref = self.heap.borrow();
            (heap_ref.car(tv), heap_ref.cdr(tv))
        };

        // Write the car
        self.write_tagged(car_tv, out, emitted);

        // Handle the cdr
        if cdr_tv.is_null() {
            // End of proper list
        } else if cdr_tv.is_pair() {
            // Native heap pair — continue list
            let addr = cdr_tv.raw() as usize;
            if let Some(&label) = self.labels.get(&addr)
                && emitted.get(&addr).copied().unwrap_or(false)
            {
                out.push_str(&format!(" . #{}#", label));
                return;
            }
            out.push(' ');
            self.write_tagged_list_contents(cdr_tv, out, emitted);
        } else {
            // Non-pair, non-null — dotted pair
            out.push_str(" . ");
            self.write_tagged(cdr_tv, out, emitted);
        }
    }

    /// Check for quote shorthand on a native heap pair
    fn check_quote_shorthand_tagged(&self, tv: TaggedValue) -> Option<String> {
        let (car_tv, cdr_tv) = {
            let heap_ref = self.heap.borrow();
            (heap_ref.car(tv), heap_ref.cdr(tv))
        };

        // Check if car is a quote-like symbol
        let prefix = {
            let heap_ref = self.heap.borrow();
            match heap_ref.get_symbol_or_identifier_name(car_tv) {
                Some("quote") => Some("'"),
                Some("quasiquote") => Some("`"),
                Some("unquote") => Some(","),
                Some("unquote-splicing") => Some(",@"),
                _ => None,
            }
        };
        let prefix = prefix?;

        // Check that cdr is a single-element list (pair with null cdr)
        if cdr_tv.is_pair() {
            let (inner_car, inner_cdr) = {
                let heap_ref = self.heap.borrow();
                (heap_ref.car(cdr_tv), heap_ref.cdr(cdr_tv))
            };
            if inner_cdr.is_null() {
                let mut result = String::from(prefix);
                let mut local_emitted = HashMap::new();
                self.write_tagged(inner_car, &mut result, &mut local_emitted);
                return Some(result);
            }
        }

        None
    }
}

// =============================================================================
// Public entry points — TaggedValue-based (no conversion needed)
// =============================================================================

/// Format a TaggedValue for write (machine-readable, datum labels for circular only)
pub fn format_write_tagged(tv: TaggedValue, heap: &RefCell<Heap>) -> String {
    let mut writer = DatumLabelWriter::new(false, false, heap);
    writer.find_circular_tagged(tv);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_tagged(tv, &mut output, &mut emitted);
    output
}

/// Format a TaggedValue for display (human-readable, datum labels for circular only)
pub fn format_display_tagged(tv: TaggedValue, heap: &RefCell<Heap>) -> String {
    let mut writer = DatumLabelWriter::new(true, false, heap);
    writer.find_circular_tagged(tv);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_tagged(tv, &mut output, &mut emitted);
    output
}

/// Format a TaggedValue for write-shared (datum labels for all shared)
pub(super) fn format_write_shared_tagged(tv: TaggedValue, heap: &RefCell<Heap>) -> String {
    let mut writer = DatumLabelWriter::new(false, true, heap);
    writer.find_circular_tagged(tv);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_tagged(tv, &mut output, &mut emitted);
    output
}

/// Format a TaggedValue for write-simple (no datum labels)
pub(super) fn format_write_simple_tagged(tv: TaggedValue, heap: &RefCell<Heap>) -> String {
    format_simple_recursive_tagged(tv, heap, false)
}

// =============================================================================
// Simple recursive formatting (no circular structure handling)
// =============================================================================

/// Format a TaggedValue without circular structure handling (primary path).
/// Handles immediate types and native heap types directly.
fn format_simple_recursive_tagged(tv: TaggedValue, heap: &RefCell<Heap>, display: bool) -> String {
    // Immediate types
    if tv.is_fixnum() {
        return tv.as_fixnum_unchecked().to_string();
    }
    if tv.is_null() {
        return "()".to_string();
    }
    if tv.is_boolean() {
        return if tv.as_bool_unchecked() { "#t" } else { "#f" }.to_string();
    }
    if tv.is_char() {
        if display {
            return tv.as_char_unchecked().to_string();
        }
        // TaggedValue::Display handles named chars (#\space, #\newline, etc.)
        return tv.to_string();
    }
    if tv.is_eof() {
        return "#<eof>".to_string();
    }
    if tv == TaggedValue::UNSPECIFIED {
        return "#<unspecified>".to_string();
    }

    // Native heap pair
    if tv.is_pair() {
        let (car_tv, cdr_tv) = {
            let hr = heap.borrow();
            (hr.car(tv), hr.cdr(tv))
        };
        let mut result = String::from("(");
        result.push_str(&format_simple_recursive_tagged(car_tv, heap, display));
        format_simple_list_tail(cdr_tv, &mut result, heap, display);
        result.push(')');
        return result;
    }

    // Native heap vector
    if tv.is_vector() {
        let elements: Vec<TaggedValue> = {
            let hr = heap.borrow();
            hr.vector_slice(tv).to_vec()
        };
        let mut result = String::from("#(");
        for (i, elem_tv) in elements.iter().enumerate() {
            if i > 0 {
                result.push(' ');
            }
            result.push_str(&format_simple_recursive_tagged(*elem_tv, heap, display));
        }
        result.push(')');
        return result;
    }

    // Native heap string
    if tv.is_string() {
        let hr = heap.borrow();
        let chars = hr.get_string_chars(tv);
        if display {
            return chars.iter().collect();
        }
        let mut result = String::from("\"");
        for &c in chars {
            match c {
                '"' => result.push_str("\\\""),
                '\\' => result.push_str("\\\\"),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                c if c.is_control() => {
                    result.push_str(&format!("\\x{:02x};", c as u32));
                }
                c => result.push(c),
            }
        }
        result.push('"');
        return result;
    }

    // HeapObjectData leaf types
    let hr = heap.borrow();
    let mut result = String::new();
    format_leaf_object(tv, &hr, display, &mut result);
    result
}

/// Format remaining list elements (cdr chain) in simple mode.
fn format_simple_list_tail(
    start: TaggedValue,
    result: &mut String,
    heap: &RefCell<Heap>,
    display: bool,
) {
    let mut cdr_tv = start;
    loop {
        if cdr_tv.is_null() {
            break;
        } else if cdr_tv.is_pair() {
            let (car, next) = {
                let hr = heap.borrow();
                (hr.car(cdr_tv), hr.cdr(cdr_tv))
            };
            result.push(' ');
            result.push_str(&format_simple_recursive_tagged(car, heap, display));
            cdr_tv = next;
        } else {
            // Non-pair, non-null — dotted pair
            result.push_str(" . ");
            result.push_str(&format_simple_recursive_tagged(cdr_tv, heap, display));
            break;
        }
    }
}
