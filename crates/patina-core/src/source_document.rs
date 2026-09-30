//! Retained diagnostic text. Locations own a document handle, so code can
//! report its source after the parser and its source map have gone away.

use crate::source_map::source_lines;
use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Identity belongs to an input, not its name: two REPL submissions are
/// different documents even when both are called `<repl>`.
pub struct SourceDocument {
    id: u64,
    text: RwLock<DocumentText>,
}

#[derive(Debug, Default)]
struct DocumentText {
    text: String,
    lines: Vec<usize>,
    first_line: u32,
    expansions: HashMap<(u32, u32), Vec<String>>,
}

impl std::fmt::Debug for SourceDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceDocument")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SourceDocument {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for SourceDocument {}

impl SourceDocument {
    pub fn new(text: String) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let mut contents = DocumentText {
            text,
            first_line: 1,
            ..Default::default()
        };
        contents.reindex();
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            text: RwLock::new(contents),
        }
    }

    pub fn get_line(&self, line: u32) -> Option<String> {
        let text = self.text.read().unwrap();
        let index = line.checked_sub(text.first_line)? as usize;
        let start = *text.lines.get(index)?;
        let end = text
            .lines
            .get(index + 1)
            .copied()
            .unwrap_or(text.text.len());
        Some(
            text.text[start..end]
                .trim_end_matches(['\r', '\n'])
                .to_owned(),
        )
    }

    pub(crate) fn push_line(&self, line: u32, input: &str) {
        let mut text = self.text.write().unwrap();
        if text.text.is_empty() {
            text.first_line = line;
        }
        let offset = text.text.len();
        text.lines.push(offset);
        text.text.push_str(input);
        if !input.ends_with(['\r', '\n']) {
            text.text.push('\n');
        }
    }

    pub(crate) fn forget_old_lines(&self, max_bytes: usize, keep_from_line: u32) {
        let mut text = self.text.write().unwrap();
        if text.text.len() <= max_bytes {
            return;
        }
        let mut count = 0;
        let mut cut = 0;
        for line in source_lines(&text.text) {
            if text.text.len() - cut <= max_bytes / 2
                || text.first_line + count >= keep_from_line
                || !line.ends_with(['\r', '\n'])
            {
                break;
            }
            cut += line.len();
            count += 1;
        }
        if cut == 0 {
            return;
        }
        text.text.drain(..cut);
        text.first_line += count;
        let first = text.first_line;
        text.expansions.retain(|&(line, _), _| line >= first);
        text.reindex();
    }

    pub(crate) fn record_expansion(&self, line: u32, column: u32, name: String) {
        self.text
            .write()
            .unwrap()
            .expansions
            .entry((line, column))
            .or_default()
            .push(name);
    }

    pub(crate) fn expansions(&self, line: u32, column: u32) -> Option<Vec<String>> {
        self.text
            .read()
            .unwrap()
            .expansions
            .get(&(line, column))
            .cloned()
    }
}

impl DocumentText {
    fn reindex(&mut self) {
        self.lines.clear();
        let mut offset = 0;
        for line in source_lines(&self.text) {
            self.lines.push(offset);
            offset += line.len();
        }
    }
}

/// An exclusive end position, counted in characters, like the reader.
/// Line/column coordinates remain stable when streaming input is compacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    pub document: std::sync::Arc<SourceDocument>,
    pub end_line: u32,
    pub end_column: u32,
    /// Expansion history belongs to this syntax occurrence, not its text
    /// position: one written macro template can be instantiated many times.
    pub expansion_chain: Option<std::sync::Arc<[String]>>,
}

#[cfg(test)]
mod tests {
    use crate::SourceMap;

    #[test]
    fn locations_retain_documents_and_expansion_chains_across_same_named_inputs() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<crate::SourceLocation>();
        let mut first = SourceMap::new();
        first.set_primary_source("<repl>");
        first.set_source_text("α\r\n  missing\rfinal".into());
        let location = first.location("<repl>", 2, 3, 2, 10);
        first.record_expansion(&location, "outer".into());
        drop(first);
        let mut next = SourceMap::new();
        next.set_primary_source("<repl>");
        next.set_source_text("other\n  different".into());
        let other = next.location("<repl>", 2, 3, 2, 12);
        next.record_expansion(&other, "inner".into());
        assert_ne!(location.span, other.span);
        assert_eq!(
            next.format_context(&location).as_deref(),
            Some("   2 |   missing\n         ^^^^^^^")
        );
        assert_eq!(next.get_expansions(&location), Some(vec!["outer".into()]));
        assert_eq!(next.get_expansions(&other), Some(vec!["inner".into()]));
        assert_eq!(
            location
                .span
                .as_ref()
                .unwrap()
                .document
                .get_line(3)
                .as_deref(),
            Some("final")
        );
    }

    #[test]
    fn retained_stream_positions_survive_compaction_without_quoting_wrong_text() {
        let mut map = SourceMap::new();
        map.set_primary_source("<stdin>");
        map.push_source_line(40, "old\r\n");
        let old = map.location("<stdin>", 40, 1, 40, 4);
        map.record_expansion(&old, "old-macro".into());
        map.push_source_line(41, "  λ-name\r");
        let kept = map.location("<stdin>", 41, 3, 41, 9);
        map.forget_old_source_lines(1, 41);
        map.push_source_line(42, "next\n");
        assert!(map.format_context(&old).is_none());
        assert!(map.get_expansions(&old).is_none());
        assert_eq!(
            map.format_context(&kept).as_deref(),
            Some("  41 |   λ-name\n         ^^^^^^")
        );
        assert_eq!(map.get_line(42).as_deref(), Some("next"));
    }
}
