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
//! One `#[test]` per suite, so a load failure in one cannot hide the
//! others. The guard test at the bottom makes adding an entry (or a
//! recorded reason not to) a condition of bundling a library at all.

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
#[allow(clippy::too_many_arguments)]
fn assert_suite<B: Backend>(
    interp: &Interpreter<B>,
    backend: &str,
    library: &str,
    imports: &str,
    body: &str,
    expected_failures: i64,
    min_assertions: i64,
) -> i64 {
    let label = format!("{library} on {backend}");
    let program = harness_program(imports, body);
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
///
/// `imports`/`body` default to importing the suite and calling `(run-tests)`;
/// a table row overrides them when the suite needs a pinned environment.
fn check_suite(
    library: &str,
    imports: &str,
    body: &str,
    expected_failures: i64,
    min_assertions: i64,
) {
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
        imports,
        body,
        expected_failures,
        min_assertions,
    );

    let vm = Interpreter::new(VmBackend::new());
    vm.backend().add_library_search_path(root);
    let vm_assertions = assert_suite(
        &vm,
        "vm",
        library,
        imports,
        body,
        expected_failures,
        min_assertions,
    );

    assert_eq!(
        tw_assertions, vm_assertions,
        "[{library}] the backends disagree on how many assertions ran"
    );
}

/// One `#[test]` per suite, plus a `COVERED` table naming the bundled
/// library each suite tests — the guard test below walks `lib/` against it,
/// so a library can only join the bundle by adding a row here or a recorded
/// reason there.
macro_rules! suite_tests {
    // Internal: one runner call, with or without a row's imports/body override.
    (@run $library:expr, $failures:expr, $floor:expr) => {
        check_suite($library, $library, "(run-tests)", $failures, $floor)
    };
    (@run $library:expr, $failures:expr, $floor:expr, $imports:expr, $body:expr) => {
        check_suite($library, $imports, $body, $failures, $floor)
    };
    ($(($name:ident, $covers:expr, $library:expr, $expected_failures:expr, $min_assertions:expr $(, $imports:expr, $body:expr)?)),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                suite_tests!(@run $library, $expected_failures, $min_assertions $(, $imports, $body)?);
            }
        )*
        /// The bundled libraries whose upstream suites run above.
        const COVERED: &[&str] = &[$($covers),*];
    };
}

// The expectations table: (test name, library under test, suite library,
// expected failures, minimum assertions run). Not a skip list — a non-zero
// failure entry is a defect in *our* port, recorded rather than hidden, and
// the assertion floor is the count the suite ran when the expectation was
// recorded (2026-08-12; chibi rows and SRFI 14, 2026-08-19).
suite_tests! {
    (srfi_151_bitwise, "srfi 151", "(srfi 151 test)", 0, 145),
    (srfi_143_fixnum, "srfi 143", "(srfi 143 test)", 0, 141),
    (srfi_132_sort, "srfi 132", "(srfi 132 test)", 0, 221),
    (srfi_133_vector, "srfi 133", "(srfi 133 test)", 0, 93),
    (srfi_113_set, "srfi 113", "(srfi 113 test)", 0, 253),
    // Verbatim, and it needed no adaptation because SRFI 162's constants are
    // exported from `(srfi 128)` — which is where SRFI 162 says to put them.
    // Adapting the import list, as SRFI 125's and 130's suites needed, would
    // have been the wrong fix here.
    (srfi_128_comparator, "srfi 128", "(srfi 128 test)", 0, 170),
    // Adapted, not verbatim: its two chibi char-set imports were replaced by
    // `(srfi 14)`, test bodies untouched. Why, in
    // scheme_tests/upstream/README.md.
    (srfi_130_string, "srfi 130", "(srfi 130 test)", 0, 219),
    (srfi_158_generator, "srfi 158", "(srfi 158 test)", 0, 76),
    // The other adapted suite: imports adapted, test bodies untouched. Why,
    // in scheme_tests/upstream/README.md.
    (srfi_125_hash_table, "srfi 125", "(srfi 125 test)", 0, 74),
    // Its first run caught ucs-range->char-set discarding its base set —
    // see the note at that procedure in lib/srfi/14.scm.
    (srfi_14_char_set, "srfi 14", "(srfi 14 test)", 0, 72),
    // Philip Bewig's own suite for SRFI 41, verbatim from chibi's R7RS
    // adaptation of it — the same file the reference implementation ships
    // with, so it exercises `stream-match` too (ours is chibi's macro over
    // the reference body; see lib/srfi/PROVENANCE.md).
    (srfi_41_stream, "srfi 41", "(srfi 41 test)", 0, 186),
    // The one row whose non-zero failure count is *not* ours, against the
    // convention above. chibi's suite does `(list-queue-append! x …)` and
    // then asserts `x` is unchanged; SRFI 117 says of that procedure "it is
    // an error to assume anything about the contents of the list-queues
    // after the procedure returns", so the assertion tests chibi's own
    // choice rather than the specification. Patina ships the SRFI's
    // reference implementation, which reuses the storage the spec frees it
    // to reuse — and Larceny's suite, which does not make that assumption,
    // passes 40 of 40.
    (srfi_117_list_queue, "srfi 117", "(srfi 117 test)", 1, 34),
    (srfi_127_lseq, "srfi 127", "(srfi 127 test)", 0, 109),
    // chibi's suite, which exercises the interface and so runs against the
    // SRFI's reference implementation unchanged. Larceny's is richer (345
    // assertions) and finds 8 failures in the comparator section — see
    // PROVENANCE.md and triage family 29.
    (srfi_116_ilist, "srfi 116", "(srfi 116 test)", 0, 196),
    // Upstream's own suite for the implementation bundled here — the
    // distribution's top-level `srfi-134-tests.scm`, which is the one matched
    // to `srfi/134.sld`. (The `ideque-2list` directory ships a near-identical
    // suite for the *other* implementation; taking that one instead, as this
    // row first did, buys nothing and breaks the version-matching this tree
    // otherwise keeps.) Wrapped in a `(srfi 134 test)` library with its import
    // block replaced; test bodies unmodified. See scheme_tests/upstream/README.
    //
    // It duplicates Larceny's `ideque` suite almost exactly — the two exercise
    // the same 55 procedures — so this row is not extra *coverage*. It is
    // coverage that runs: Larceny's suite is a separate lane nothing in CI
    // invokes, and this one is a cargo test.
    (srfi_134_ideque, "srfi 134", "(srfi 134 test)", 0, 119),
    (srfi_135_text, "srfi 135", "(srfi 135 test)", 0, 1071),
    // Verbatim; runnable since `(scheme flonum)` is bundled.
    //
    // It also costs about 27s, which is this whole binary's runtime — the
    // other 20 suites finish in a few seconds and wait for it — most of that
    // few is SRFI 135, at ~3s on the tree-walker. What costs 27s is 14 of
    // its 224 assertions, drawing 131 000 random numbers between them; what
    // they buy is the chi-squared power the suite over-provisions on purpose
    // (alpha 1e-5), and nothing else. They are *not* what reaches
    // `mrg32k3a-random-large` — the cheap `test-random` loop above them walks
    // `n` to 2^204, so it takes the bignum path on ~172 of its own iterations
    // for free. Trimming the draws would mean editing a verbatim suite and
    // would cost only that statistical power, which is a real thing to weigh
    // and not the coverage argument an earlier version of this comment made.
    (srfi_27_random, "srfi 27", "(srfi 27 test)", 0, 224),
    // The chibi suites are from the same pinned snowballs as the bundled
    // libraries themselves (lib/chibi/PROVENANCE.md), restored after the
    // corpus stopped vendoring packages Patina bundles — which had silently
    // dropped these suites from everything that runs. string-test is
    // verbatim; the other three had their inline framework shims replaced
    // by the real (chibi test), documented in scheme_tests/upstream/README.md.
    (chibi_string, "chibi string", "(chibi string-test)", 0, 52),
    (chibi_optional, "chibi optional", "(chibi optional-test)", 0, 11),
    // Two of its assertions expect ANSI escapes in edits->string/color's
    // output unconditionally, but (chibi term ansi) initializes
    // ansi-escapes-enabled? from ANSI_ESCAPES_ENABLED/TERM — so a bare run
    // is green under a developer's xterm and red under CI's dumb TERM.
    // Pin the parameter to the setting the suite assumes.
    (chibi_diff, "chibi diff", "(chibi diff-test)", 0, 7,
        "(chibi diff-test) (chibi term ansi)",
        "(parameterize ((ansi-escapes-enabled? #t)) (run-tests))"),
    (chibi_term_ansi, "chibi term ansi", "(chibi term ansi-test)", 0, 234),
}

/// Whole trees under `lib/` whose libraries are accounted for by another
/// mechanism, with the reason. A tree not named here is in scope for the
/// guard by default, so bundling a new tree cannot silently reopen the hole
/// the guard exists to close — that is how the original one opened: the
/// corpus builder's bundled-package exclusion (L4) dropped five chibi
/// suites from everything that runs, and nothing noticed until an audit.
const NO_SUITE_TREES: &[(&str, &str)] = &[
    (
        "scheme",
        "the R7RS surface (gated by the chibi R7RS suite) plus alias libraries whose backing SRFIs this table covers, drift-checked in r7rs_large_aliases.rs",
    ),
    (
        "r6rs",
        "awaits its own vendored suite — Track L §L5.3; this entry retires when it lands",
    ),
    (
        "rnrs",
        "one-line shims over lib/r6rs, checked by r6rs_rnrs_shims.rs",
    ),
];

/// Every library bundled in a `lib/` tree not excused above either has its
/// upstream suite in the table above or a recorded reason here for not
/// having one. Before this guard, "add a suite when Patina bundles the
/// library" was a comment.
///
/// Each reason is a claim to re-verify when circumstances change, not a
/// permanent pass — several name the event that retires them.
const NO_SUITE: &[(&str, &str)] = &[
    (
        "srfi 1",
        "upstream suite imports (chibi), chibi's implementation core",
    ),
    ("srfi 8", "no upstream suite exists (receive: one macro)"),
    (
        "srfi 23",
        "re-export shim over (scheme base)'s error; reexport_shims.rs pins it",
    ),
    (
        "srfi 135 kernel8",
        "SRFI 135's text representation, not a library anyone imports: `(srfi 135)` selects it and upstream ships no suite for it separately. `srfi/135/test.sld` exercises it through the whole of `(srfi 135)`, 1030 assertions",
    ),
    (
        "srfi 144",
        "no upstream suite runs under (chibi test) — the SRFI's own targets a Larceny-family harness and chibi's tests chibi's API; retires when either is ported or reconciled, see scheme_tests/upstream/README.md",
    ),
    (
        "srfi 33",
        "rename shim over (srfi 151), whose suite runs above; srfi_151_bitwise.rs pins the renames",
    ),
    (
        "srfi 60",
        "rename shim over (srfi 151), whose suite runs above; srfi_151_bitwise.rs pins the MSB-first deviations",
    ),
    (
        "srfi 69",
        "upstream suite imports (chibi), chibi's implementation core",
    ),
    (
        "srfi 98",
        "re-export shim over (scheme process-context); reexport_shims.rs pins it",
    ),
    ("srfi 111", "no upstream suite exists (boxes)"),
    (
        "srfi 142",
        "rename shim over (srfi 151), whose suite runs above; srfi_151_bitwise.rs pins the bitwise-if swap",
    ),
    (
        "chibi filesystem",
        "upstream suite opens a raw file descriptor before its directory tests, hitting the bundled FFI stub outside any test form, which aborts the run; add the suite when FFI lands",
    ),
    (
        "chibi test",
        "the framework itself — exercised by every suite above, the self-check below, and the chibi R7RS gate",
    ),
];

#[test]
fn every_bundled_library_has_a_suite_or_a_recorded_reason() {
    let all = common::shipped_libraries(&repo_root().join("lib"));
    assert!(!all.is_empty(), "found no bundled .sld files — wrong root?");

    let excused_trees: Vec<&str> = NO_SUITE_TREES.iter().map(|(tree, _)| *tree).collect();
    let bundled: Vec<String> = all
        .iter()
        .filter(|name| !excused_trees.contains(&name[0].as_str()))
        .map(|name| name.join(" "))
        .collect();

    let excused: Vec<&str> = NO_SUITE.iter().map(|(lib, _)| *lib).collect();
    for lib in &bundled {
        let covered = COVERED.contains(&lib.as_str());
        let has_excuse = excused.contains(&lib.as_str());
        assert!(
            covered || has_excuse,
            "({lib}) is bundled but its upstream suite does not run: add a suite_tests! \
             row (see scheme_tests/upstream/README.md) or a NO_SUITE reason"
        );
        assert!(
            !(covered && has_excuse),
            "({lib}) is both suited and excused — delete its NO_SUITE entry"
        );
    }
    // A stale excuse is as misleading as a missing one — for libraries and
    // for whole trees alike.
    for lib in &excused {
        assert!(
            bundled.iter().any(|b| b == lib),
            "NO_SUITE names ({lib}), which is not bundled — delete the entry"
        );
    }
    for tree in &excused_trees {
        assert!(
            all.iter().any(|name| name[0] == *tree),
            "NO_SUITE_TREES names ({tree}), which holds no bundled libraries — delete the entry"
        );
    }
}

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
