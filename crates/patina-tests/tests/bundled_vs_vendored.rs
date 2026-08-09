//! Guards Patina's bundled library ports against the canonical upstream copies
//! in `compat/vendor/`.
//!
//! For libraries Patina ships itself, the vendored package is not just a corpus
//! subject — it is the **canonical reference** the bundled port is measured
//! against. Ports drift: someone adds an export to `lib/srfi/1.sld` that
//! upstream does not have, or upstream gains one we never pick up, and the
//! divergence is only discovered when a third-party package mysteriously fails
//! to find a binding.
//!
//! This is a **ratchet, not a skip list**. Every known divergence is recorded
//! below with the reason it exists. A new divergence fails the build; closing a
//! recorded one also fails the build, so the record cannot silently rot.
//!
//! Unlike the corpus sweep — which is an out-of-band measurement and
//! deliberately not a CI gate — this runs in routine CI, because bundled
//! libraries are part of what Patina promises.
//!
//! TODO(L4): this whole file is interim scaffolding. The target state is that
//! each bundled port is a faithful import of upstream, the **bundled** version
//! is canonical, and the vendored duplicate is removed from `compat/vendor/`
//! entirely — at which point there is nothing to compare and this test, along
//! with the `bundled_by_patina` manifest flag, should be deleted. It exists
//! only to keep the duplicated state safe until then. See
//! `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` § L4.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A bundled library, its vendored counterpart, and the divergences we accept.
struct Guarded {
    /// Library name as written in `define-library`, space-separated.
    library: &'static str,
    /// `.sld` under `lib/`, relative to the repo root.
    bundled: &'static str,
    /// `.sld` under `compat/vendor/`, relative to the repo root.
    vendored: &'static str,
    /// Exports Patina has that upstream does not, and why.
    only_bundled: &'static [&'static str],
    /// Exports upstream has that Patina does not, and why.
    only_vendored: &'static [&'static str],
    /// Why the divergence is acceptable. Documentation, not logic.
    rationale: &'static str,
}

const CXR: &[&str] = &[
    "caaaar", "caaadr", "caadar", "caaddr", "caadr", "caar", "cadaar", "cadadr", "cadar", "caddar",
    "cadddr", "caddr", "cadr", "cdaaar", "cdaadr", "cdaar", "cdadar", "cdaddr", "cdadr", "cdar",
    "cddaar", "cddadr", "cddar", "cdddar", "cddddr", "cddr",
];

const CHIBI_TEST_EXTRA: &[&str] = &[
    "current-column-width",
    "current-test-applier",
    "current-test-comparator",
    "current-test-epsilon",
    "current-test-filters",
    "current-test-group",
    "current-test-group-filters",
    "current-test-group-removers",
    "current-test-group-reporter",
    "current-test-removers",
    "current-test-reporter",
    "current-test-skipper",
    "current-test-verbosity",
    "test-equal",
    "test-exit",
    "test-failure-count",
    "test-get-name!",
    "test-group",
    "test-group-inc!",
    "test-group-name",
    "test-group-push!",
    "test-group-ref",
    "test-group-set!",
    "test-propagate-info",
    "test-run",
    "test-syntax-error",
];

const GUARDED: &[Guarded] = &[
    Guarded {
        library: "srfi 1",
        bundled: "lib/srfi/1.sld",
        vendored: "compat/vendor/srfi-1/srfi/1.sld",
        only_bundled: &["list-copy", "make-list"],
        only_vendored: CXR,
        rationale: "Different R7RS adaptations of the same Olin Shivers reference (74 lines of \
                    real drift, the rest whitespace). Upstream comments out make-list/list-copy \
                    and leans on (scheme base); Patina exports them. Upstream also re-exports the \
                    c[ad]+r accessors; Patina keeps those in (scheme cxr).",
    },
    Guarded {
        library: "srfi 8",
        bundled: "lib/srfi/8.sld",
        vendored: "compat/vendor/srfi-8/srfi/8.sld",
        only_bundled: &[],
        only_vendored: &[],
        rationale: "Identical export sets.",
    },
    Guarded {
        library: "srfi 69",
        bundled: "lib/srfi/69.sld",
        vendored: "compat/vendor/srfi-69/srfi/69.sld",
        only_bundled: &[],
        only_vendored: &[],
        rationale: "Identical export sets.",
    },
    Guarded {
        library: "srfi 128",
        bundled: "lib/srfi/128.sld",
        vendored: "compat/vendor/srfi-128/srfi/128.sld",
        only_bundled: &[],
        only_vendored: &["%salt%"],
        rationale: "%salt% is an internal hash parameter upstream exposes; not part of the SRFI \
                    128 specification and deliberately not re-exported.",
    },
    Guarded {
        library: "chibi test",
        bundled: "lib/chibi/test.sld",
        vendored: "compat/vendor/chibi-test/chibi/test.sld",
        only_bundled: &["test-increment-failed", "test-increment-passed"],
        only_vendored: CHIBI_TEST_EXTRA,
        rationale: "Patina implements the subset of (chibi test) the R7RS suite needs, plus two \
                    counter hooks of its own. The upstream extras are test-runner configuration \
                    and grouping that nothing in the corpus has required yet — expect this gap to \
                    shrink as corpus packages ask for them.",
    },
];

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/patina-tests
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

/// Collect every identifier in every `(export ...)` clause of a `.sld`.
fn exports_of(path: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let bytes: Vec<char> = text.chars().collect();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while let Some(pos) = text[i..].find("(export") {
        let start = i + pos;
        // Walk to the matching close paren.
        let mut depth = 0usize;
        let mut end = start;
        for (k, c) in bytes.iter().enumerate().skip(text[..start].chars().count()) {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = k;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body: String = bytes[text[..start].chars().count() + "(export".len()..end]
            .iter()
            .collect();
        for line in body.lines() {
            let line = line.split(';').next().unwrap_or("");
            for tok in line.split_whitespace() {
                let tok = tok.trim_matches(|c| c == '(' || c == ')');
                if !tok.is_empty() {
                    out.insert(tok.to_string());
                }
            }
        }
        i = end.max(start + 1);
    }
    out
}

#[test]
fn test_bundled_ports_match_vendored_reference() {
    let root = repo_root();
    let mut problems = Vec::new();

    for g in GUARDED {
        let bundled_path = root.join(g.bundled);
        let vendored_path = root.join(g.vendored);
        assert!(
            bundled_path.exists(),
            "({}) is listed as bundled but {} is missing",
            g.library,
            g.bundled
        );
        assert!(
            vendored_path.exists(),
            "({}) has no vendored reference at {} — if the corpus dropped it, remove the entry \
             here too rather than leaving a dangling guard",
            g.library,
            g.vendored
        );

        let bundled = exports_of(&bundled_path);
        let vendored = exports_of(&vendored_path);

        let actual_only_bundled: BTreeSet<_> = bundled.difference(&vendored).cloned().collect();
        let actual_only_vendored: BTreeSet<_> = vendored.difference(&bundled).cloned().collect();
        let expected_only_bundled: BTreeSet<String> =
            g.only_bundled.iter().map(|s| s.to_string()).collect();
        let expected_only_vendored: BTreeSet<String> =
            g.only_vendored.iter().map(|s| s.to_string()).collect();

        let mut local: Vec<String> = Vec::new();
        for name in actual_only_bundled.difference(&expected_only_bundled) {
            local.push(format!(
                "({}) exports `{name}` which the vendored reference does not. If intentional, add \
                 it to only_bundled with a reason.",
                g.library
            ));
        }
        for name in actual_only_vendored.difference(&expected_only_vendored) {
            local.push(format!(
                "({}) is missing `{name}`, which the vendored reference exports. Either implement \
                 it or record it in only_vendored with a reason.",
                g.library
            ));
        }
        // A closed gap must be recorded too, or the expectations rot.
        for name in expected_only_bundled.difference(&actual_only_bundled) {
            local.push(format!(
                "({}) no longer exports `{name}`, but it is still listed in only_bundled — remove \
                 the stale entry.",
                g.library
            ));
        }
        for name in expected_only_vendored.difference(&actual_only_vendored) {
            local.push(format!(
                "({}) now exports `{name}` — remove it from only_vendored, the gap is closed.",
                g.library
            ));
        }

        if !local.is_empty() {
            problems.push(format!(
                "({}) -- currently accepted divergence is: {}\n      {}",
                g.library,
                g.rationale,
                local.join("\n      ")
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "bundled libraries drifted from their vendored reference:\n  - {}",
        problems.join("\n  - ")
    );
}

/// The guard list must cover everything the corpus marks `bundled_by_patina`,
/// so bundling a new library cannot silently escape this check.
#[test]
fn test_every_bundled_corpus_package_is_guarded() {
    let root = repo_root();
    let manifest =
        std::fs::read_to_string(root.join("compat/vendor/MANIFEST.json")).expect("corpus manifest");
    let guarded: BTreeSet<&str> = GUARDED.iter().map(|g| g.library).collect();

    // Deliberately string-scraped rather than pulling in a JSON dependency for
    // one field: find each "library" paired with "bundled_by_patina": true.
    let mut flagged = BTreeSet::new();
    for chunk in manifest.split("\"library\": \"").skip(1) {
        let name = chunk.split('"').next().unwrap_or("");
        let obj_end = chunk.find("\n    },").unwrap_or(chunk.len());
        if chunk[..obj_end].contains("\"bundled_by_patina\": true") {
            flagged.insert(name.to_string());
        }
    }
    assert!(
        !flagged.is_empty(),
        "manifest parse found no bundled packages — parser broken?"
    );

    let missing: Vec<_> = flagged
        .iter()
        .filter(|n| !guarded.contains(n.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "these libraries are bundled and vendored but not guarded: {missing:?}"
    );
}
