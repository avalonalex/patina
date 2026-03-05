//! Source map for tracking source locations of parsed TaggedValues
//!
//! During parsing, each significant TaggedValue (lists, identifiers, etc.)
//! is recorded with its source position. This allows the desugarer and
//! evaluator to attach source locations to CoreExpr/CpsExpr nodes.

use patina_core::TaggedValue;
use patina_core::error::SourceLocation;
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
            expansion_records: HashMap::new(),
        }
    }

    /// Store the source text for caret-style error display.
    pub fn set_source_text(&mut self, text: String) {
        self.source_text = Some(text);
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

    /// Return the ordered list of macro names expanded at this location, if any.
    pub fn get_expansions(&self, loc: &SourceLocation) -> Option<&[String]> {
        self.expansion_records
            .get(&(loc.line, loc.column))
            .map(|v| v.as_slice())
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
}
