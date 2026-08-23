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

/// The rows of a two-level document: `(<doc> ... (<rows> <row> ...) ...)`.
///
/// Both files this harness owns are that shape — `results.scm` and
/// `EXCLUSIONS.scm` — so the walk, and the wording of the error when the
/// head is wrong, live here rather than once per reader. Other sections
/// (`results.scm` carries `backend`) come back in `sections` for the caller
/// to scan; only the row list is common enough to name.
pub fn document_rows(
    source: &str,
    doc_head: &str,
    rows_head: &str,
    heap: &SharedHeap,
) -> Result<(Vec<TaggedValue>, Vec<TaggedValue>), String> {
    let forms = parse_all(source, heap)?;
    let sections = forms
        .first()
        .and_then(|f| tagged_form(*f, doc_head, heap))
        .ok_or_else(|| format!("not a {doc_head} file"))?;
    let rows = sections
        .iter()
        .filter_map(|s| tagged_form(*s, rows_head, heap))
        .flatten()
        .collect();
    Ok((sections, rows))
}

/// The argument of the `key` clause among a row's fields.
///
/// A row is a list of one-argument clauses — `((slug "x") (reason ffi) ...)`
/// — which both readers walk the same way. Naming it keeps the accessor
/// closures out of each `parse_*_row`.
pub fn row_field(fields: &[TaggedValue], key: &str, heap: &SharedHeap) -> Option<TaggedValue> {
    fields.iter().find_map(|f| clause_argument(*f, key, heap))
}

/// The `key` clause's argument as a string, if it is one.
pub fn row_string(fields: &[TaggedValue], key: &str, heap: &SharedHeap) -> Option<String> {
    row_field(fields, key, heap).and_then(|tv| string_value(tv, heap))
}

/// The `key` clause's argument as a symbol name, if it is one.
pub fn row_symbol(fields: &[TaggedValue], key: &str, heap: &SharedHeap) -> Option<String> {
    row_field(fields, key, heap).and_then(|tv| symbol_name(tv, heap))
}

/// The pooled bodies of a `(cond-expand ...)` form's clauses, or `None` if
/// `tv` is not one. Each clause is `(condition form ...)`; the condition is
/// skipped, never evaluated — corpus collectors pool every branch as a
/// deliberate over-approximation — and a clause with no body contributes
/// nothing.
pub fn cond_expand_bodies(tv: TaggedValue, heap: &SharedHeap) -> Option<Vec<TaggedValue>> {
    let clauses = tagged_form(tv, "cond-expand", heap)?;
    let mut bodies = Vec::new();
    for clause in clauses {
        if let Some(elems) = list_elements(clause, heap)
            && elems.len() > 1
        {
            bodies.extend_from_slice(&elems[1..]);
        }
    }
    Some(bodies)
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
