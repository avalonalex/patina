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
//! Each suite runs on **both backends**, and two numbers are checked per run:
//!
//! - the failure count, exactly and in both directions — a regression fails,
//!   and so does a fix until the expectation is lowered;
//! - a floor on the assertions actually run. `(chibi test)` honors
//!   `TEST_FILTER`/`TEST_GROUP_FILTER`/`TEST_GROUP_REMOVE` from the
//!   environment, so without the floor a filtered or skip-everything run
//!   reports zero failures and looks like a pass.
//!
//! The count comes from a wrapper installed around `current-test-reporter`
//! rather than from a patch to the framework — the bundled `(chibi test)`
//! stays verbatim (see `scheme_tests/upstream/README.md`).
//!
//! Add a `suite_test!` entry when Patina bundles a library whose upstream
//! suite exists; one `#[test]` per suite, so a load failure in one cannot
//! hide the others.

mod common;

use common::repo_root;
use patina_interpreter::{Interpreter, TreeWalkInterpreter};
use patina_primitives::primitives::io::datum_writer::format_display_tagged;
use patina_runtime::Backend;
use patina_vm::VmBackend;
use std::path::PathBuf;

fn upstream_root() -> PathBuf {
    repo_root().join("scheme_tests").join("upstream")
}

/// Wrap `body` (test forms or a suite's `(run-tests)`) so the program's value
/// is `"<failures> <assertions-run>"`.
///
/// Both numbers are the framework's own: the failure count is its global
/// parameter, and the assertion count is taken by wrapping the reporter it
/// calls once per non-skipped assertion.
fn harness_program(imports: &str, body: &str) -> String {
    format!(
        "(import (scheme base) (chibi test) {imports}) \
         (define assertions-run 0) \
         (current-test-reporter \
           (let ((default (current-test-reporter))) \
             (lambda (status info) \
               (unless (eq? status 'SKIP) \
                 (set! assertions-run (+ assertions-run 1))) \
               (default status info)))) \
         {body} \
         (string-append (number->string (test-failure-count)) \
                        \" \" \
                        (number->string assertions-run))"
    )
}

/// Run `program` and return `(failures, assertions_run)`.
fn counts_on<B: Backend>(interp: &Interpreter<B>, label: &str, program: &str) -> (i64, i64) {
    let value = interp
        .eval_program(program)
        .unwrap_or_else(|e| panic!("[{label}] failed to run: {e}"));
    let text = format_display_tagged(value, interp.backend().global_env().heap());
    let mut parts = text.split_whitespace();
    let mut next_int = || {
        parts
            .next()
            .and_then(|n| n.parse::<i64>().ok())
            .unwrap_or_else(|| panic!("[{label}] expected two integers, got {text:?}"))
    };
    (next_int(), next_int())
}

/// Run one suite on one backend, hold it to the expectations table, and
/// return how many assertions ran (for the cross-backend agreement check).
fn assert_suite<B: Backend>(
    interp: &Interpreter<B>,
    backend: &str,
    library: &str,
    expected_failures: i64,
    min_assertions: i64,
) -> i64 {
    let label = format!("{library} on {backend}");
    let program = harness_program(library, "(run-tests)");
    let (failures, assertions) = counts_on(interp, &label, &program);
    assert!(
        assertions >= min_assertions,
        "[{label}] ran {assertions} assertions, expected at least {min_assertions} — \
         a filter (TEST_FILTER et al.) or a framework change is skipping tests"
    );
    if failures != expected_failures {
        let verdict = if failures > expected_failures {
            "a regression"
        } else {
            "fixed, lower the expectation"
        };
        panic!("[{label}] {failures} failures, was {expected_failures} — {verdict}");
    }
    assertions
}

/// Run one suite on both backends. Failure counts agree by construction (both
/// are pinned to the same expectation), so the assertions-run count is the one
/// number the backends could silently diverge on — compare it too.
fn check_suite(library: &str, expected_failures: i64, min_assertions: i64) {
    let root = upstream_root();
    assert!(
        root.is_dir(),
        "upstream suites missing at {} — see its README",
        root.display()
    );

    let tw = TreeWalkInterpreter::new_tree_walker();
    tw.backend()
        .evaluator()
        .add_library_search_path(root.clone());
    let tw_assertions = assert_suite(
        &tw,
        "tree-walker",
        library,
        expected_failures,
        min_assertions,
    );

    let vm = Interpreter::new(VmBackend::new());
    vm.backend().add_library_search_path(root);
    let vm_assertions = assert_suite(&vm, "vm", library, expected_failures, min_assertions);

    assert_eq!(
        tw_assertions, vm_assertions,
        "[{library}] the backends disagree on how many assertions ran"
    );
}

macro_rules! suite_test {
    ($name:ident, $library:expr, $expected_failures:expr, $min_assertions:expr) => {
        #[test]
        fn $name() {
            check_suite($library, $expected_failures, $min_assertions);
        }
    };
}

// The expectations table: (test name, library, expected failures, minimum
// assertions run). Not a skip list — a non-zero failure entry is a defect in
// *our* port, recorded rather than hidden, and the assertion floor is the
// count the suite ran when the expectation was recorded (2026-08-12).
suite_test!(srfi_151_bitwise, "(srfi 151 test)", 0, 145);
suite_test!(srfi_143_fixnum, "(srfi 143 test)", 0, 141);
suite_test!(srfi_132_sort, "(srfi 132 test)", 0, 221);
suite_test!(srfi_133_vector, "(srfi 133 test)", 0, 93);
suite_test!(srfi_113_set, "(srfi 113 test)", 0, 253);
// The only suite here not copied verbatim: its two chibi char-set imports were
// replaced by `(srfi 14)`, test bodies untouched. Why, in
// scheme_tests/upstream/README.md.
suite_test!(srfi_130_string, "(srfi 130 test)", 0, 219);
suite_test!(srfi_158_generator, "(srfi 158 test)", 0, 76);

/// Proves the harness can actually report a failure — and actually counts.
///
/// Without this, the suite tests are vacuous twice over: a
/// `test-failure-count` that always returned 0 would look like a clean run,
/// and an assertion counter stuck at 0 would never trip a floor set at or
/// below the real count. One deliberate failure and two passes through the
/// same harness have to come back as exactly `1 3`.
///
/// It prints one `FAIL:` line per backend during the run. That is the point.
#[test]
fn test_harness_reports_failures_and_counts() {
    fn expect_one_failure_of_three<B: Backend>(interp: &Interpreter<B>, label: &str) {
        let body = "(test-begin \"deliberate\") (test 1 2) (test 3 3) (test 5 5) (test-end)";
        assert_eq!(
            counts_on(interp, label, &harness_program("", body)),
            (1, 3),
            "a failing assertion must be counted, or the real suites' numbers mean nothing"
        );
    }

    let tw = TreeWalkInterpreter::new_tree_walker();
    expect_one_failure_of_three(&tw, "self-check on tree-walker");

    let vm = Interpreter::new(VmBackend::new());
    expect_one_failure_of_three(&vm, "self-check on vm");
}
