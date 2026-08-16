//! Small helpers for walking parsed s-expressions (`TaggedValue` trees).
//!
//! The harness reads `package.scm` files and its own results file with
//! Patina's real parser — the corpus format *is* s-expressions, so no other
//! serialization layer is needed.

use patina_core::{SharedHeap, TaggedValue};

/// Parse every top-level form in `source`.
pub fn parse_all(source: &str, heap: &SharedHeap) -> Result<Vec<TaggedValue>, String> {
    patina_frontend::Parser::new_with_heap(source, heap.clone())
        .map_err(|e| e.to_string())?
        .parse_all()
        .map_err(|e| e.to_string())
}

/// A proper list's elements, or None for any other value.
pub fn list_elements(tv: TaggedValue, heap: &SharedHeap) -> Option<Vec<TaggedValue>> {
    heap.borrow().list_to_vec(tv)
}

/// The symbol name of `tv`, if it is a symbol.
pub fn symbol_name(tv: TaggedValue, heap: &SharedHeap) -> Option<String> {
    heap.borrow().get_symbol_name(tv).map(|s| s.to_string())
}

/// The contents of `tv`, if it is a string.
pub fn string_value(tv: TaggedValue, heap: &SharedHeap) -> Option<String> {
    heap.borrow().get_string_contents(tv)
}

/// Is `tv` a list whose head is the symbol `head`? Returns the tail if so.
pub fn tagged_form(tv: TaggedValue, head: &str, heap: &SharedHeap) -> Option<Vec<TaggedValue>> {
    let elems = list_elements(tv, heap)?;
    let (first, rest) = elems.split_first()?;
    if symbol_name(*first, heap).as_deref() == Some(head) {
        Some(rest.to_vec())
    } else {
        None
    }
}

/// The single argument of a one-argument clause: `(path "x")` → `"x"`'s
/// datum. Most `package.scm` clauses have this shape, and pairing
/// `tagged_form` with `.first()` at each of them reads worse than naming it.
pub fn clause_argument(tv: TaggedValue, head: &str, heap: &SharedHeap) -> Option<TaggedValue> {
    tagged_form(tv, head, heap)?.first().copied()
}

/// Render a library name list like `(chibi match)` or `(srfi 1)` as the
/// canonical space-joined form `"chibi match"` / `"srfi 1"`.
pub fn library_name(tv: TaggedValue, heap: &SharedHeap) -> Option<String> {
    let elems = list_elements(tv, heap)?;
    if elems.is_empty() {
        return None;
    }
    let mut parts = Vec::with_capacity(elems.len());
    for e in elems {
        // A library-name component is a symbol or a non-negative integer
        // (R7RS §5.6); anything else means this is not a library name.
        parts.push(match symbol_name(e, heap) {
            Some(s) => s,
            None => e.as_fixnum()?.to_string(),
        });
    }
    Some(parts.join(" "))
}

/// Escape a string for embedding in Scheme string-literal syntax.
pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}
