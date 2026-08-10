//! Run each bundled SRFI's *own* reference test suite against Patina.
//!
//! The suites in `scheme_tests/upstream/` are the specification authors' tests,
//! not ours. That matters more than the count: a hand-written test only covers
//! the cases its author imagined, and for a library we ported, that author is
//! the same person who made the porting mistakes. SRFI 151's suite is 145
//! assertions against the 13 in `srfi_151_bitwise.rs`.
//!
//! Running them at all is a consequence of adopting upstream `(chibi test)` —
//! the hand-written subset it replaced could not express `test-group` or report
//! a failure count, so these suites had nothing to run on.
//!
//! Add an entry when Patina bundles a library whose upstream suite exists.

use patina_interpreter::TreeWalkInterpreter;
use std::path::PathBuf;

/// A bundled library and the reference suite that exercises it.
const SUITES: &[(&str, &str)] = &[
    ("(srfi 151 test)", "SRFI 151 bitwise"),
    ("(srfi 143 test)", "SRFI 143 fixnum"),
];

fn upstream_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("scheme_tests")
        .join("upstream")
}

/// Load a suite, run it, and report how many assertions it failed.
///
/// `(chibi test)` tracks this itself, so the count comes from the framework
/// rather than from scraping its output.
fn failures_in(library: &str) -> i64 {
    let root = upstream_root();
    assert!(
        root.is_dir(),
        "upstream suites missing at {} — see its README",
        root.display()
    );

    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.backend().evaluator().add_library_search_path(root);

    let program =
        format!("(import (scheme base) (chibi test) {library}) (run-tests) (test-failure-count)");
    let value = interp
        .eval_program(&program)
        .unwrap_or_else(|e| panic!("{library} failed to run: {e:?}"));
    interp
        .display_tagged(value)
        .parse()
        .expect("test-failure-count should return an integer")
}

#[test]
fn test_upstream_suites_pass() {
    let mut failed = Vec::new();
    for (library, description) in SUITES {
        let failures = failures_in(library);
        if failures != 0 {
            failed.push(format!("{description}: {failures} assertion(s)"));
        }
    }
    assert!(
        failed.is_empty(),
        "reference suites reported failures:\n  - {}",
        failed.join("\n  - ")
    );
}

/// Proves the harness can actually report a failure.
///
/// Without this, `test_upstream_suites_pass` is vacuous: a `test-failure-count`
/// that always returned 0 — or a `run-tests` that quietly did nothing — would
/// look exactly like a clean run. A deliberately failing assertion through the
/// same path has to come back as 1.
///
/// It prints one `FAIL:` line during the run. That is the point of it.
#[test]
fn test_harness_reports_failures() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let value = interp
        .eval_program(
            "(import (scheme base) (chibi test)) \
             (test-begin \"deliberate\") (test 1 2) (test-end) (test-failure-count)",
        )
        .expect("self-check should run");
    assert_eq!(
        interp.display_tagged(value),
        "1",
        "a failing assertion must be counted, or a zero from the real suites means nothing"
    );
}
