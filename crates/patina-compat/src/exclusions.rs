//! The opt-out list: packages whose failure is not a measurement of Patina.
//!
//! The corpus contains packages that cannot pass for reasons that have
//! nothing to do with this interpreter — they need a foreign-function
//! interface, their dependency is one the corpus licence policy declined to
//! vendor, or their source is
//! invalid Scheme that only chibi's lenient reader accepts. Counting those
//! against the score makes the headline measure the corpus rather than
//! Patina, and leaves the bundling queue pointing at work nobody can do.
//!
//! Three properties keep this from becoming a place to hide failures:
//!
//! 1. **An excluded package still runs.** Exclusion decides whether a result
//!    counts, never whether it is measured. `results.scm` is unchanged by
//!    this file — the snapshot stays the measurement, this list is the
//!    policy, and the report is the join.
//! 2. **Every entry states what it expects.** An exclusion carries the
//!    status bucket it was written against; when the package stops producing
//!    that bucket the entry has drifted and the report says so instead of
//!    quietly absorbing the change. An entry that starts passing is drift
//!    too — that is how one gets retired.
//! 3. **Both numbers are reported.** The raw "N of M" never goes away; the
//!    scoped number sits beside it with the exclusion count in between.

use crate::sexp;
use patina_core::SharedHeap;
use std::path::Path;

/// Why a package does not count. A closed set: a reason that does not fit
/// one of these is a reason to think again, not to add prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// Needs a foreign-function interface — an `include-shared` shared
    /// object, `(foreign c)`, or a C-backed `(chibi …)` library. Deferred by
    /// Track L §3, and a non-goal for the C-shim spelling specifically: the
    /// eventual answer is a Rust FFI, which will not make these packages
    /// load unchanged.
    Ffi,
    /// Depends on a package the corpus does not vendor. `build_corpus.py`
    /// excludes packages whose licence it cannot establish, so this is our
    /// own vendoring policy reaching the score, not a Patina limitation.
    DependencyNotVendored,
    /// The package's own source is invalid Scheme. Every entry names the
    /// construct and the implementation that agrees with us in rejecting it;
    /// accepting it would mean widening the language to match one reader's
    /// leniency. See Track L PRD §6 "Upstream, not ours".
    UpstreamSourceDefect,
    /// The library is fine; the *test program* is not portable, or asserts
    /// behaviour no standard specifies. Kept apart from the entry above
    /// because it says something different: the thing third parties import
    /// works, and only the suite fails.
    UpstreamTestDefect,
}

impl Reason {
    pub fn key(&self) -> &'static str {
        match self {
            Reason::Ffi => "ffi",
            Reason::DependencyNotVendored => "dependency-not-vendored",
            Reason::UpstreamSourceDefect => "upstream-source-defect",
            Reason::UpstreamTestDefect => "upstream-test-defect",
        }
    }

    fn from_key(key: &str) -> Option<Reason> {
        match key {
            "ffi" => Some(Reason::Ffi),
            "dependency-not-vendored" => Some(Reason::DependencyNotVendored),
            "upstream-source-defect" => Some(Reason::UpstreamSourceDefect),
            "upstream-test-defect" => Some(Reason::UpstreamTestDefect),
            _ => None,
        }
    }

    /// The heading this reason gets in the report, in declaration order.
    pub const ALL: [Reason; 4] = [
        Reason::Ffi,
        Reason::DependencyNotVendored,
        Reason::UpstreamSourceDefect,
        Reason::UpstreamTestDefect,
    ];

    pub fn heading(&self) -> &'static str {
        match self {
            Reason::Ffi => "Needs a foreign-function interface",
            Reason::DependencyNotVendored => "Dependency not vendored (corpus licence policy)",
            Reason::UpstreamSourceDefect => "Upstream source defect",
            Reason::UpstreamTestDefect => "Upstream test defect",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Exclusion {
    pub slug: String,
    pub reason: Reason,
    /// The status bucket this entry was written against. A package that
    /// produces a different one has drifted — see the module docs.
    pub expect: String,
    /// One line saying what is actually wrong, specific enough that a reader
    /// can check it without re-deriving the diagnosis.
    pub note: String,
}

/// Read the opt-out list. A missing file is an empty list, not an error: the
/// harness must run on a checkout that has not got one.
pub fn load(path: &Path, heap: &SharedHeap) -> Result<Vec<Exclusion>, String> {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {}", path.display(), e)),
    };
    parse(&source, heap)
}

pub fn parse(source: &str, heap: &SharedHeap) -> Result<Vec<Exclusion>, String> {
    let forms = sexp::parse_all(source, heap)?;
    let top = forms
        .first()
        .and_then(|f| sexp::tagged_form(*f, "patina-compat-exclusions", heap))
        .ok_or("not a patina-compat-exclusions file")?;

    let mut out = Vec::new();
    for section in top {
        let Some(rows) = sexp::tagged_form(section, "exclusions", heap) else {
            continue;
        };
        for row in rows {
            out.push(parse_row(row, heap)?);
        }
    }

    let mut seen = std::collections::BTreeSet::new();
    for e in &out {
        if !seen.insert(e.slug.clone()) {
            return Err(format!("duplicate exclusion for {}", e.slug));
        }
    }
    Ok(out)
}

fn parse_row(row: patina_core::TaggedValue, heap: &SharedHeap) -> Result<Exclusion, String> {
    let fields = sexp::list_elements(row, heap).ok_or("malformed exclusion row")?;
    let field = |key: &str| fields.iter().find_map(|f| sexp::tagged_form(*f, key, heap));
    let string_field = |key: &str| {
        field(key).and_then(|rest| rest.first().and_then(|tv| sexp::string_value(*tv, heap)))
    };
    let symbol_field = |key: &str| {
        field(key).and_then(|rest| rest.first().and_then(|tv| sexp::symbol_name(*tv, heap)))
    };

    let slug = string_field("slug").ok_or("exclusion row without slug")?;
    let reason_key = symbol_field("reason").ok_or_else(|| format!("{}: no reason", slug))?;
    let reason = Reason::from_key(&reason_key)
        .ok_or_else(|| format!("{}: unknown reason `{}`", slug, reason_key))?;
    let expect = symbol_field("expect").ok_or_else(|| format!("{}: no expect", slug))?;
    if !crate::run::Status::KEYS.contains(&expect.as_str()) {
        return Err(format!("{}: unknown expected status `{}`", slug, expect));
    }
    let note = string_field("note").ok_or_else(|| format!("{}: no note", slug))?;
    if note.trim().is_empty() {
        return Err(format!("{}: empty note", slug));
    }
    Ok(Exclusion {
        slug,
        reason,
        expect,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heap() -> SharedHeap {
        patina_core::new_shared_heap()
    }

    const SAMPLE: &str = r#"
(patina-compat-exclusions
 (version 1)
 (exclusions
  ((slug "chibi-mecab") (reason ffi) (expect out-of-scope)
   (note "chibi/mecab.sld is (include-shared \"mecab\") over libmecab"))))
"#;

    #[test]
    fn reads_an_entry() {
        let got = parse(SAMPLE, &heap()).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].slug, "chibi-mecab");
        assert_eq!(got[0].reason, Reason::Ffi);
        assert_eq!(got[0].expect, "out-of-scope");
        assert!(got[0].note.contains("include-shared"));
    }

    #[test]
    fn rejects_an_unknown_reason() {
        let src = SAMPLE.replace("(reason ffi)", "(reason because-i-said-so)");
        let err = parse(&src, &heap()).unwrap_err();
        assert!(err.contains("unknown reason"), "{}", err);
    }

    #[test]
    fn rejects_an_unknown_expected_status() {
        let src = SAMPLE.replace("(expect out-of-scope)", "(expect mostly-fine)");
        let err = parse(&src, &heap()).unwrap_err();
        assert!(err.contains("unknown expected status"), "{}", err);
    }

    /// The note is the whole justification, so an entry without one is not an
    /// exclusion, it is a deletion with extra steps.
    #[test]
    fn rejects_an_entry_with_no_note() {
        let src = SAMPLE.replace(
            r#"(note "chibi/mecab.sld is (include-shared \"mecab\") over libmecab")"#,
            r#"(note "")"#,
        );
        let err = parse(&src, &heap()).unwrap_err();
        assert!(err.contains("empty note"), "{}", err);
    }

    #[test]
    fn rejects_a_duplicate_slug() {
        let src = SAMPLE.replace(
            "(exclusions\n",
            "(exclusions\n  ((slug \"chibi-mecab\") (reason ffi) (expect out-of-scope) (note \"x\"))\n",
        );
        let err = parse(&src, &heap()).unwrap_err();
        assert!(err.contains("duplicate exclusion"), "{}", err);
    }

    fn workspace_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace root")
            .to_path_buf()
    }

    /// The committed list must parse, or a run silently scores every package.
    #[test]
    fn the_committed_list_parses() {
        let root = workspace_root();
        let list = load(&root.join("compat/EXCLUSIONS.scm"), &heap()).expect("EXCLUSIONS.scm");
        assert!(!list.is_empty(), "the committed list should not be empty");
    }

    /// An entry naming a package the corpus does not have excludes nothing
    /// while reading as though it did. The report says so at run time; this
    /// says so in CI, which is where a corpus rebuild would drop a package.
    #[test]
    fn every_excluded_slug_is_a_vendored_package() {
        let root = workspace_root();
        let list = load(&root.join("compat/EXCLUSIONS.scm"), &heap()).expect("EXCLUSIONS.scm");
        for e in &list {
            let dir = root.join("compat/vendor").join(&e.slug);
            assert!(
                dir.is_dir(),
                "compat/EXCLUSIONS.scm excludes `{}`, which is not in compat/vendor/. \
                 Delete the entry, or fix the slug.",
                e.slug
            );
        }
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let got = load(Path::new("/nonexistent/EXCLUSIONS.scm"), &heap()).unwrap();
        assert!(got.is_empty());
    }
}
