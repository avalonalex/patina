//! Retained diagnostic text. Locations own a document handle, so code can
//! report its source after the parser and its source map have gone away.

use crate::source_map::source_lines;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

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
    pub expansion_chain: Option<ExpansionChain>,
}

/// The macros one occurrence of syntax was expanded through, outermost
/// first. A chain is a list of shared links, so extending one allocates one
/// link, and giving it to every node of an expansion is a reference count.
///
/// Each link records the top-level form whose expansion made it (the
/// desugarer numbers them), so that an expansion can tell its own chains
/// from one an earlier form left on syntax they share: `eval` expands one
/// quoted datum again and again, and a macro that splices the datum's pairs
/// into its output, as `case` does its clauses, reaches what the earlier
/// expansions stamped. It starts afresh there: extending those chains made
/// each `eval`'s memory and time grow with every one before it (#612). A
/// chain a macro's template holds is the exception ([`Self::for_template`]).
#[derive(Clone)]
pub struct ExpansionChain(Arc<ChainLink>);

struct ChainLink {
    name: Arc<str>,
    parent: Option<ExpansionChain>,
    len: usize,
    form: u64,
}

impl ExpansionChain {
    /// The form number of a chain a macro's template holds.
    pub const TEMPLATE: u64 = 0;

    /// `parent` extended by an expansion of the macro `name`, made while
    /// expanding the top-level form numbered `form`.
    pub fn extend(parent: Option<&ExpansionChain>, name: Arc<str>, form: u64) -> Self {
        Self(Arc::new(ChainLink {
            name,
            parent: parent.cloned(),
            len: parent.map_or(0, ExpansionChain::len) + 1,
            form,
        }))
    }

    /// How many expansions the chain records: at least one.
    pub fn len(&self) -> usize {
        self.0.len
    }

    /// Never: a chain records at least one expansion.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The top-level form whose expansion made the newest link.
    pub fn form(&self) -> u64 {
        self.0.form
    }

    /// The macro names, outermost first.
    pub fn names(&self) -> Vec<String> {
        let mut names = Vec::with_capacity(self.len());
        let mut link = Some(self);
        while let Some(chain) = link {
            names.push(chain.0.name.to_string());
            link = chain.0.parent.as_ref();
        }
        names.reverse();
        names
    }

    /// An identity for the newest link, as long as the chain is held.
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    /// This chain as a macro's template holds it, for the syntax each use of
    /// the macro introduces from it: every use extends it, whichever form
    /// the use is in. The template's copy is never stamped, so nothing
    /// accumulates on it, and the uses of a macro another macro's expansion
    /// defined keep that expansion in their chains (`def-bad → bad`).
    pub fn for_template(&self) -> Self {
        Self(Arc::new(ChainLink {
            name: self.0.name.clone(),
            parent: self.0.parent.clone(),
            len: self.0.len,
            form: Self::TEMPLATE,
        }))
    }
}

/// Two chains are equal when they record the same expansions in the same
/// order, whichever forms made them.
impl PartialEq for ExpansionChain {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        let (mut left, mut right) = (Some(self), Some(other));
        while let (Some(l), Some(r)) = (left, right) {
            if Arc::ptr_eq(&l.0, &r.0) {
                return true;
            }
            if l.0.name != r.0.name {
                return false;
            }
            left = l.0.parent.as_ref();
            right = r.0.parent.as_ref();
        }
        true
    }
}

impl Eq for ExpansionChain {}

impl std::fmt::Debug for ExpansionChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

/// A chain is as long as the expansion was deep. Release it a link at a time
/// rather than by recursion through each link's parent.
impl Drop for ChainLink {
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(chain) = next {
            next = match Arc::try_unwrap(chain.0) {
                Ok(mut link) => link.parent.take(),
                Err(_) => None,
            };
        }
    }
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
    fn chains_share_their_links_and_compare_by_names() {
        use super::ExpansionChain;
        use std::sync::Arc;
        let outer = ExpansionChain::extend(None, Arc::from("case"), 1);
        let inner = ExpansionChain::extend(Some(&outer), Arc::from("let"), 1);
        assert_eq!((outer.len(), inner.len()), (1, 2));
        assert_eq!(inner.names(), ["case", "let"]);
        assert_eq!(inner.form(), 1);
        // Made separately, by another form: the same expansions.
        let again = ExpansionChain::extend(
            Some(&ExpansionChain::extend(None, Arc::from("case"), 2)),
            Arc::from("let"),
            2,
        );
        assert_eq!(inner, again);
        assert_ne!(inner, outer);
        assert_ne!(
            inner,
            ExpansionChain::extend(Some(&outer), Arc::from("if"), 1)
        );
        assert_eq!(format!("{inner:?}"), r#"["case", "let"]"#);
    }

    /// A chain a million expansions long drops without a frame per link.
    #[test]
    fn a_long_chain_drops_without_recursion() {
        use super::ExpansionChain;
        use std::sync::Arc;
        let name: Arc<str> = Arc::from("peel");
        let mut chain = ExpansionChain::extend(None, name.clone(), 1);
        for _ in 1..1_000_000 {
            chain = ExpansionChain::extend(Some(&chain), name.clone(), 1);
        }
        assert_eq!(chain.len(), 1_000_000);
        drop(chain);
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
