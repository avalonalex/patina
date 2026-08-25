//! Source map for tracking source locations of parsed TaggedValues
//!
//! During parsing, each significant TaggedValue (lists, identifiers, etc.)
//! is recorded with its source position. This allows the desugarer and
//! evaluator to attach source locations to CoreExpr/CpsExpr nodes.

use patina_core::error::SourceLocation;
use patina_core::{GcFreedBits, SharedHeap, TaggedValue};
use std::cell::RefCell;
use std::collections::HashMap;

/// Maps TaggedValue raw bits to their source locations.
///
/// Since TaggedValue is a NaN-boxed u64, we use the raw bits as keys.
/// This is safe because equal TaggedValues have equal raw bits.
#[derive(Debug, Default)]
pub struct SourceMap {
    locations: HashMap<u64, SourceLocation>,
    /// The full source text, used for pretty error formatting (caret display).
    /// Populated by `Parser::new_with_source_map`.
    source_text: Option<String>,
    /// The name the parser was given for that text — a file path when the
    /// program came from one, `<eval>`/`<repl>` otherwise. Populated with
    /// `source_text`; the desugarer resolves a top-level relative `include`
    /// beside it.
    primary_source: Option<String>,
    /// Macro expansion chain records, keyed by (line, column) of the call site.
    /// Each entry is an ordered list of macro names expanded at that location
    /// (outermost first, matching expansion sequence).
    expansion_records: HashMap<(u32, u32), Vec<String>>,
}

impl SourceMap {
    /// Create a new empty source map
    pub fn new() -> Self {
        Self {
            locations: HashMap::new(),
            source_text: None,
            primary_source: None,
            expansion_records: HashMap::new(),
        }
    }

    /// Store the source text for caret-style error display.
    pub fn set_source_text(&mut self, text: String) {
        self.source_text = Some(text);
    }

    /// Record where the source text came from (see `primary_source`).
    pub fn set_primary_source(&mut self, name: &str) {
        self.primary_source = Some(name.to_string());
    }

    /// The name the parser was given for the current source text, if any.
    pub fn primary_source(&self) -> Option<&str> {
        self.primary_source.as_deref()
    }

    /// Return the (1-indexed) line from the stored source text, if available.
    pub fn get_line(&self, line: u32) -> Option<&str> {
        let text = self.source_text.as_deref()?;
        text.lines().nth((line as usize).saturating_sub(1))
    }

    /// Format a caret-style error context block for a source location.
    ///
    /// Returns a string like:
    /// ```text
    ///    1 | (define (foo) x)
    ///                     ^
    /// ```
    pub fn format_context(&self, loc: &SourceLocation) -> Option<String> {
        let line_text = self.get_line(loc.line)?;
        let col = (loc.column as usize).saturating_sub(1); // 0-indexed
        let caret_len = loc.length.unwrap_or(1).max(1) as usize;
        let prefix = format!("{:>4} | ", loc.line);
        let indent = " ".repeat(prefix.len() + col);
        let carets = "^".repeat(caret_len);
        Some(format!("{}{}\n{}{}", prefix, line_text, indent, carets))
    }

    /// Record a source location for a TaggedValue
    pub fn record(&mut self, tv: TaggedValue, loc: SourceLocation) {
        self.locations.insert(tv.raw_bits(), loc);
    }

    /// Look up the source location for a TaggedValue
    pub fn get(&self, tv: TaggedValue) -> Option<&SourceLocation> {
        self.locations.get(&tv.raw_bits())
    }

    /// Number of entries in the source map
    pub fn len(&self) -> usize {
        self.locations.len()
    }

    /// Whether the source map is empty
    pub fn is_empty(&self) -> bool {
        self.locations.is_empty()
    }

    /// Record that a macro with the given name was expanded at this location.
    pub fn record_expansion(&mut self, loc: &SourceLocation, macro_name: String) {
        self.expansion_records
            .entry((loc.line, loc.column))
            .or_default()
            .push(macro_name);
    }

    /// Iterate over all recorded source locations.
    pub fn iter_locations(&self) -> impl Iterator<Item = &SourceLocation> {
        self.locations.values()
    }

    /// Return the ordered list of macro names expanded at this location, if any.
    pub fn get_expansions(&self, loc: &SourceLocation) -> Option<&[String]> {
        self.expansion_records
            .get(&(loc.line, loc.column))
            .map(|v| v.as_slice())
    }

    /// Drop the entries for slots the GC reclaimed (`GC_DESIGN.md` §9.1): a
    /// reused slot must not inherit the old datum's source location.
    /// `expansion_records` is keyed by source position, not raw bits, so it
    /// stays valid and is untouched.
    pub fn prune_freed(&mut self, freed: &[u64]) {
        for bits in freed {
            self.locations.remove(bits);
        }
    }

    /// Forget all location entries. The overflow fallback for
    /// [`SourceMap::prune_freed`]: a missing location degrades a diagnostic,
    /// a stale one misattributes it.
    pub fn clear_locations(&mut self) {
        self.locations.clear();
    }
}

/// Drain the slots the GC reclaimed since the last call and drop their
/// entries from `source_map` (`GC_DESIGN.md` §9.1).
///
/// Call between evaluating one top-level form and parsing the next: sweeps
/// can only happen during evaluation, and raw-bits lookups only during the
/// desugaring of a later form, so pruning at the form boundary closes the
/// staleness window completely. Cheap when no collection ran (one empty
/// drain, no map borrow).
pub fn prune_freed_locations(heap: &SharedHeap, source_map: &RefCell<SourceMap>) {
    match heap.borrow_mut().take_gc_freed_bits() {
        GcFreedBits::Exact(freed) if freed.is_empty() => {}
        GcFreedBits::Exact(freed) => source_map.borrow_mut().prune_freed(&freed),
        GcFreedBits::Overflowed => source_map.borrow_mut().clear_locations(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_source_map_basic() {
        let mut sm = SourceMap::new();
        assert!(sm.is_empty());

        let tv = TaggedValue::fixnum(42);
        let loc = SourceLocation {
            source: Arc::from("test.scm"),
            line: 1,
            column: 5,
            length: Some(2),
        };
        sm.record(tv, loc.clone());

        assert_eq!(sm.len(), 1);
        assert!(!sm.is_empty());

        let retrieved = sm.get(tv).unwrap();
        assert_eq!(retrieved.line, 1);
        assert_eq!(retrieved.column, 5);
    }

    #[test]
    fn test_source_map_missing() {
        let sm = SourceMap::new();
        assert!(sm.get(TaggedValue::fixnum(99)).is_none());
    }

    #[test]
    fn prune_freed_drops_reclaimed_entries_only() {
        use patina_core::{Collector, GcRoots, GcVisitor, MarkSweepCollector};

        // A root provider keeping one of the two datums alive.
        struct Keep(TaggedValue);
        impl GcRoots for Keep {
            fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
                visitor.visit(self.0);
            }
        }

        let heap = patina_core::new_shared_heap();
        heap.borrow_mut().enable_gc_freed_tracking();
        let live = heap
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let dead = heap
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);

        let loc = |line| SourceLocation {
            source: Arc::from("test.scm"),
            line,
            column: 1,
            length: None,
        };
        let sm = RefCell::new(SourceMap::new());
        sm.borrow_mut().record(live, loc(1));
        sm.borrow_mut().record(dead, loc(2));

        {
            let mut h = heap.borrow_mut();
            MarkSweepCollector::new().collect(&mut h, &[&Keep(live)]);
        }
        prune_freed_locations(&heap, &sm);

        let sm = sm.borrow();
        assert_eq!(sm.get(live).unwrap().line, 1);
        assert!(
            sm.get(dead).is_none(),
            "reclaimed slot must not keep its old location"
        );
    }
}
