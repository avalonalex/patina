//! Diagnostic documents and compatibility snapshots of parsed syntax nodes.
//! The compiler reads provenance from the heap, where sweep removes it before
//! a slot can be reused. Source locations retain their own document handles.

use crate::error::SourceLocation;
use crate::source_document::{SourceDocument, SourceSpan};
use crate::{GcFreedBits, SharedHeap, TaggedValue};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// A character-based source position using R7RS 7.1.1 line endings.
/// Keep `after_cr` across input chunks: CRLF starts one line even when its
/// characters arrive separately, or the preceding text has been discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceCursor {
    pub line: u32,
    pub column: u32,
    pub after_cr: bool,
}

impl SourceCursor {
    pub const START: Self = Self {
        line: 1,
        column: 1,
        after_cr: false,
    };

    pub fn advance(&mut self, ch: char) {
        if ch == '\r' || (ch == '\n' && !self.after_cr) {
            self.line = self.line.saturating_add(1);
            self.column = 1;
        } else if ch != '\n' {
            self.column = self.column.saturating_add(1);
        }
        self.after_cr = ch == '\r';
    }
}

/// Split complete source text at LF, CRLF or bare CR, keeping each ending.
/// Like `split_inclusive`, an ending at EOF adds no empty trailing line.
pub fn source_lines(mut text: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let end = text.find(['\r', '\n']).map_or(text.len(), |at| {
            at + if text[at..].starts_with("\r\n") { 2 } else { 1 }
        });
        let (line, rest) = text.split_at(end);
        text = rest;
        Some(line)
    })
}

/// Holds a primary source document and a snapshot of parsed node locations.
/// Interned symbols cannot represent distinct occurrences. Program parsers
/// use unique syntax identifiers, and heap provenance is authoritative.
#[derive(Debug, Default)]
pub struct SourceMap {
    locations: HashMap<u64, SourceLocation>,
    document: Option<Arc<SourceDocument>>,
    /// The name the parser was given for that text — a file path when the
    /// program came from one, `<eval>`/`<repl>` otherwise. Populated with
    /// `source_text`; the desugarer resolves a top-level relative `include`
    /// beside it.
    primary_source: Option<Arc<str>>,
}

impl SourceMap {
    /// Create a new empty source map
    pub fn new() -> Self {
        Self {
            locations: HashMap::new(),
            document: None,
            primary_source: None,
        }
    }

    /// Store the source text for caret-style error display.
    pub fn set_source_text(&mut self, text: String) {
        self.document = Some(Arc::new(SourceDocument::new(text)));
    }

    /// Retain the input behind a location, including its exclusive end.
    pub fn location(
        &self,
        source: &str,
        line: u32,
        column: u32,
        end_line: u32,
        end_column: u32,
    ) -> SourceLocation {
        let mut loc = SourceLocation {
            source: self
                .primary_source
                .as_ref()
                .filter(|name| name.as_ref() == source)
                .cloned()
                .unwrap_or_else(|| Arc::from(source)),
            line,
            column,
            length: Some(if line == end_line {
                end_column.saturating_sub(column).max(1)
            } else {
                1
            }),
            span: None,
        };
        loc.span = self.document.as_ref().map(|document| SourceSpan {
            document: document.clone(),
            end_line,
            end_column,
            expansion_chain: None,
        });
        loc
    }

    /// Append a physical source line without normalizing its line ending.
    pub fn push_source_line(&mut self, line: u32, text: &str) {
        self.document
            .get_or_insert_with(|| Arc::new(SourceDocument::new(String::new())))
            .push_line(line, text);
    }

    /// Bound streamed diagnostic text, preserving an unfinished datum's lines.
    pub fn forget_old_source_lines(&mut self, max_bytes: usize, keep_from_line: u32) {
        if let Some(document) = &self.document {
            document.forget_old_lines(max_bytes, keep_from_line);
        }
    }

    /// Record where the source text came from (see `primary_source`).
    pub fn set_primary_source(&mut self, name: &str) {
        if self.primary_source.as_deref() != Some(name) {
            self.primary_source = Some(Arc::from(name));
        }
    }

    /// The name the parser was given for the current source text, if any.
    pub fn primary_source(&self) -> Option<&str> {
        self.primary_source.as_deref()
    }

    /// Return the (1-indexed) line from the stored source text, if available.
    pub fn get_line(&self, line: u32) -> Option<String> {
        self.document.as_ref()?.get_line(line)
    }

    /// Format a caret-style error context block for a source location.
    ///
    /// Returns a string like:
    /// ```text
    ///    1 | (define (foo) x)
    ///                     ^
    /// ```
    pub fn format_context(&self, loc: &SourceLocation) -> Option<String> {
        let line_text = if let Some(span) = &loc.span {
            span.document.get_line(loc.line)?
        } else if self
            .primary_source
            .as_deref()
            .is_none_or(|name| name == loc.source.as_ref())
        {
            self.get_line(loc.line)?
        } else {
            return None;
        };
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
        if let Some(document) = loc
            .span
            .as_ref()
            .map(|s| &s.document)
            .or(self.document.as_ref())
        {
            document.record_expansion(loc.line, loc.column, macro_name);
        }
    }

    /// Iterate over all recorded source locations.
    pub fn iter_locations(&self) -> impl Iterator<Item = &SourceLocation> {
        self.locations.values()
    }

    /// Return the ordered list of macro names expanded at this location, if any.
    pub fn get_expansions(&self, loc: &SourceLocation) -> Option<Vec<String>> {
        if let Some(chain) = loc
            .span
            .as_ref()
            .and_then(|span| span.expansion_chain.as_ref())
        {
            return Some(chain.to_vec());
        }
        loc.span
            .as_ref()
            .map(|s| &s.document)
            .or(self.document.as_ref())?
            .expansions(loc.line, loc.column)
    }

    /// Drop the entries for slots the GC reclaimed (`GC_DESIGN.md` §9.1): a
    /// reused slot must not inherit the old datum's source location.
    /// Expansion records belong to retained source documents, so they remain
    /// valid and are untouched.
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
/// Call between evaluating one top-level form and parsing the next to bound
/// the compatibility snapshot. Compiler lookups use the heap's provenance,
/// pruned directly by sweep, rather than relying on this shared drain. Cheap
/// when no collection ran (one empty drain, no map borrow).
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
    fn source_lines_and_cursor_agree_on_all_three_endings() {
        let text = "a\r\nb\rc\n\r\r\nλ";
        let lines: Vec<_> = source_lines(text).collect();
        assert_eq!(lines, ["a\r\n", "b\r", "c\n", "\r", "\r\n", "λ"]);
        let mut cursor = SourceCursor::START;
        for ch in text.chars() {
            cursor.advance(ch);
        }
        assert_eq!((cursor.line, cursor.column), (6, 2));
        assert!(source_lines("").next().is_none());
        assert_eq!(source_lines("\r\n").collect::<Vec<_>>(), ["\r\n"]);
    }

    #[test]
    fn raw_and_streamed_source_quote_and_forget_the_same_logical_lines() {
        let text = "first\r\nsecond\rthird\n\r\r\nλ last";
        let mut raw = SourceMap::new();
        raw.set_source_text(text.into());
        let mut streamed = SourceMap::new();
        for (i, line) in source_lines(text).enumerate() {
            streamed.push_source_line(i as u32 + 1, line);
        }
        for map in [&mut raw, &mut streamed] {
            for (i, expected) in ["first", "second", "third", "", "", "λ last"]
                .into_iter()
                .enumerate()
            {
                assert_eq!(map.get_line(i as u32 + 1).as_deref(), Some(expected));
            }
            let at = SourceLocation::new("test.scm", 6, 3);
            assert_eq!(
                map.format_context(&at).unwrap(),
                "   6 | λ last\n         ^"
            );
            map.forget_old_source_lines(1, 4);
            assert_eq!(map.get_line(3).as_deref(), None);
            assert_eq!(map.get_line(4).as_deref(), Some(""));
            assert_eq!(map.get_line(6).as_deref(), Some("λ last"));
        }
    }

    /// A source read a line at a time is quoted by its own line numbers, and
    /// forgetting old lines bounds what is held without dropping the line an
    /// unfinished datum began on, or the expansion records of lines kept.
    #[test]
    fn lines_read_one_at_a_time_are_quoted_and_the_oldest_forgotten() {
        let mut sm = SourceMap::new();
        let loc = |line| SourceLocation {
            source: Arc::from("<stdin>"),
            line,
            column: 1,
            length: None,
            span: None,
        };
        for line in 1..=4 {
            sm.push_source_line(line, &format!("(form {line})\r\n"));
            sm.record_expansion(&loc(line), format!("m{line}"));
        }
        assert_eq!(sm.get_line(2).as_deref(), Some("(form 2)"));
        assert_eq!(sm.get_line(5).as_deref(), None);

        sm.forget_old_source_lines(1000, 4);
        assert_eq!(
            sm.get_line(1).as_deref(),
            Some("(form 1)"),
            "within the budget"
        );

        sm.forget_old_source_lines(10, 3);
        assert_eq!(sm.get_line(2).as_deref(), None);
        assert!(sm.get_expansions(&loc(2)).is_none());
        assert_eq!(
            sm.get_line(3).as_deref(),
            Some("(form 3)"),
            "an unfinished datum's line stays"
        );
        assert_eq!(
            sm.get_expansions(&loc(3)).as_deref(),
            Some(&["m3".to_string()][..])
        );

        sm.set_source_text("x\n".to_string());
        assert_eq!(
            sm.get_line(1).as_deref(),
            Some("x"),
            "whole text starts at line 1 again"
        );
    }

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
            span: None,
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
        use crate::{Collector, GcRoots, GcVisitor, MarkSweepCollector};

        // A root provider keeping one of the two datums alive.
        struct Keep(TaggedValue);
        impl GcRoots for Keep {
            fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
                visitor.visit(self.0);
            }
        }

        let heap = crate::new_shared_heap();
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
            span: None,
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
