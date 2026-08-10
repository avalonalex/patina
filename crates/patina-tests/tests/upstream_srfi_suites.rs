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

/// A bundled library, its reference suite, and the number of assertions that
/// suite currently fails against Patina.
///
/// An expectations table, not a skip list. The count is asserted exactly in
/// both directions, so fixing a library fails this test until the number is
/// lowered — which is the point. A non-zero entry is a defect in *our* port,
/// recorded rather than hidden.
const SUITES: &[(&str, &str, i64)] = &[
    ("(srfi 151 test)", "SRFI 151 bitwise", 0),
    ("(srfi 143 test)", "SRFI 143 fixnum", 0),
    ("(srfi 132 test)", "SRFI 132 sort", 0),
    ("(srfi 133 test)", "SRFI 133 vector", 0),
    ("(srfi 113 test)", "SRFI 113 set", 0),
    // The one remaining failure is a Patina defect, not a defect in (srfi 158):
    // `current-input-port` is a plain 0-arg procedure rather than an R7RS
    // parameter object, so the suite's `(parameterize ((current-input-port ...)))`
    // fails with "expects exactly 0 arguments, got 1". Same for the output and
    // error ports. Tracked in the Track L PRD; lower this to 0 when it is fixed.
    ("(srfi 158 test)", "SRFI 158 generator", 1),
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
fn test_upstream_suites_match_expectations() {
    let mut drift = Vec::new();
    for (library, description, expected) in SUITES {
        let actual = failures_in(library);
        if actual > *expected {
            drift.push(format!(
                "{description}: {actual} failures, was {expected} — a regression"
            ));
        } else if actual < *expected {
            drift.push(format!(
                "{description}: {actual} failures, was {expected} — fixed, lower the expectation"
            ));
        }
    }
    assert!(
        drift.is_empty(),
        "reference suites drifted:\n  - {}",
        drift.join("\n  - ")
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
