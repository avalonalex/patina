//! Run each bundled SRFI's *own* reference test suite against Patina.
//!
//! The suites in `scheme_tests/upstream/` are the specification authors' tests,
//! not ours. That matters more than the count: a hand-written test only covers
//! the cases its author imagined, and for a library we ported, that author is
//! the same person who made the porting mistakes. SRFI 151's suite is 145
//! assertions against the few dozen hand-written SRFI 151 rows in
//! `tests/scheme/srfi/bitwise.scm`.
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
//! rather than from a patch to the framework — the supplied `(chibi test)`
//! stays verbatim (see `scheme_tests/upstream/README.md`).
//!
//! One `#[test]` per suite, so a load failure in one cannot hide the
//! others. The guard test at the bottom makes adding an entry (or a
//! recorded reason not to) a condition of bundling a library at all.

mod common;

use common::repo_root;
use patina_interpreter::Interpreter;
use patina_primitives::primitives::io::datum_writer::format_display_tagged;
use patina_runtime::Backend;
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
///
/// A suite reporting through a *different* framework sets `failures` from its
/// own counter, which then takes precedence — see `SRFI_64_BODY`. Leaving it
/// `#f` is what makes `(chibi test)`'s count the default rather than a
/// competing answer.
fn harness_program(imports: &str, body: &str) -> String {
    format!(
        "(import (scheme base) (chibi test) {imports}) \
         (define assertions-run 0) \
         (define failures #f) \
         (current-test-reporter \
           (let ((default (current-test-reporter))) \
             (lambda (status info) \
               (unless (eq? status 'SKIP) \
                 (set! assertions-run (+ assertions-run 1))) \
               (default status info)))) \
         {body} \
         (string-append (number->string (or failures (test-failure-count))) \
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

    // From `common`, so `test-lib/` is already on the path: every suite here
    // reports through `(chibi test)`, which Patina supplies rather than
    // bundles. `scheme_tests/upstream/` is this file's own root, holding the
    // suites themselves.
    let tw = common::tree_walker_interpreter();
    tw.backend().add_library_search_path(root.clone());
    let tw_assertions = assert_suite(
        &tw,
        "tree-walker",
        library,
        imports,
        body,
        expected_failures,
        min_assertions,
    );

    let vm = common::vm_interpreter();
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
// failure entry records a defect in our port or an explicitly documented
// upstream expectation, rather than hiding it. The assertion floor is the
// count the suite ran when the expectation was recorded (2026-08-12; chibi rows and SRFI 14, 2026-08-19).
/// The body for a suite that reports through `(srfi 64)` rather than
/// `(chibi test)`.
///
/// The default body reads `(chibi test)`'s `test-failure-count` and counts
/// assertions by wrapping its `current-test-reporter`. A SRFI 64 suite touches
/// neither, so the default would report *zero assertions and zero failures* —
/// vacuously green. This reads SRFI 64's own counters instead, which keeps the
/// rule the default follows: the numbers are always the framework's, never
/// re-derived here. `failures` is picked up by the harness's
/// `(test-failure-count)` call, which this shadows for the same reason.
///
/// **An `xpass` counts as a failure**, which is not obvious and is the whole
/// point of the expectations table: a `test-expect-fail` row that starts
/// passing is a stale quarantine, and making it *fail* is what retires it.
/// SRFI 64's own `test-exit` gates on `(and (zero? xpass) (zero? fail))`
/// (`lib/srfi/64.scm`), `scheme_suite.rs` holds `xpass` to zero separately,
/// and `patina-compat` classifies one as a wrong result — so counting only
/// `fail` here would make this file the one place in the repo that disagrees.
///
/// `assertions-run` is likewise `pass + fail + xpass + xfail`, matching
/// `scheme_suite.rs`'s `Counts::ran()`: everything that executed, with skips
/// excluded deliberately, so the floor measures the same thing in both places.
///
/// `test-runner-null` stops the suite writing a `.log` into the crate root,
/// the way `scheme_suite.rs`'s driver does.
const SRFI_64_BODY: &str = "(test-runner-current (test-runner-null)) \
     (run-tests) \
     (let ((r (test-runner-get))) \
       (set! assertions-run (+ (test-runner-pass-count r) \
                               (test-runner-fail-count r) \
                               (test-runner-xpass-count r) \
                               (test-runner-xfail-count r))) \
       (set! failures (+ (test-runner-fail-count r) \
                         (test-runner-xpass-count r))))";

/// The body for SRFI 159's suite, which writes a scratch file to run
/// `from-file` against.
///
/// Upstream's `from-file` group creates `chibi-show-test-0123456789` by
/// *relative* path — so, under `cargo test`, in the crate root — and deletes
/// it on the last line of the group. That delete does not run if an assertion
/// before it raises, which leaves an untracked file behind in the tree. This
/// deletes it unconditionally after the run, for the same reason
/// `SRFI_64_BODY` passes `test-runner-null`: a suite must not leave anything
/// in the crate root on its way out.
const SRFI_159_BODY: &str = "(run-tests) \
     (guard (e (#t #f)) (delete-file \"chibi-show-test-0123456789\"))";

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
    // scheme_tests/upstream/README.md. #204 fixes string-every's witness:
    // upstream expects #t for (string-every char->integer "aAbA"), but SRFI
    // 130 requires the final predicate result, 65. Keep that upstream row;
    // tests/scheme/srfi/string-cursors.scm asserts the specified behavior.
    (srfi_130_string, "srfi 130", "(srfi 130 test)", 1, 219),
    (srfi_158_generator, "srfi 158", "(srfi 158 test)", 0, 76),
    // Verbatim, and it needed no adaptation: its own cond-expand already
    // reaches for `(chibi test)` on anything that is not Larceny, which is
    // the framework this harness supplies.
    (srfi_115_regexp, "srfi 115", "(srfi 115 test)", 0, 85),
    // Verbatim, and it needed no adaptation: (srfi 146) became available in
    // #375, which is what made this one possible at all.
    (srfi_165_computations, "srfi 165", "(srfi 165 test)", 0, 43),
    // Upstream's own suite, with its imports lifted into the wrapper `.sld`
    // and nothing else changed; see that file. It exercises the whole
    // `(srfi 159)` surface, which is why the sub-libraries below are excused
    // rather than given rows of their own.
    (srfi_159_show, "srfi 159", "(srfi 159 test)", 0, 316,
     "(srfi 159 test) (scheme file)",
     SRFI_159_BODY),
    // Upstream tests s16 alone, and says why: "if one vector type works, they
    // all work" — the twelve `(srfi 160 <type>)` libraries are sed-expanded
    // from one template, so a template defect shows in all of them. The other
    // eleven, and `(srfi 160 base)`, are exercised by
    // tests/scheme/srfi/homogeneous-vectors.scm.
    (srfi_160_s16vector, "srfi 160 s16", "(srfi 160 test)", 0, 110),
    // Both verbatim from the SRFI's own distribution, and both passed on the
    // first run with no adaptation — unusual enough in this table to be worth
    // recording. `(srfi 146)` is the red-black tree implementation and
    // `(srfi 146 hash)` the HAMT one; they are separate libraries with
    // separate suites, so both are registered.
    //
    // These two are the first rows here whose suite reports through
    // `(srfi 64)` rather than `(chibi test)`, so the default body's
    // `current-test-reporter` wrapper never sees their assertions and would
    // count zero. The override runs the suite and then reads SRFI 64's own
    // counters instead, which is the same principle the default uses: the
    // framework's numbers, never a re-derivation. `test-runner-null` keeps the
    // suite from writing a log into the crate root, as the driver for
    // `tests/scheme/` does for the same reason.
    (srfi_146_mapping, "srfi 146", "(srfi 146 test)", 0, 97,
     "(srfi 146 test) (srfi 64)",
     SRFI_64_BODY),
    (srfi_146_hashmap, "srfi 146 hash", "(srfi 146 hash test)", 0, 77,
     "(srfi 146 hash test) (srfi 64)",
     SRFI_64_BODY),
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
    //
    // 186 → 187 when the suite was re-vendored at chibi f15b0814 (2026-08-27),
    // which fixed the `stream->list` defect this tree reported as #1181 and
    // added the bounded-prefix assertion for it. The reference body Patina
    // ships never had the defect, so the new assertion passed on arrival.
    (srfi_41_stream, "srfi 41", "(srfi 41 test)", 0, 187),
    // Another upstream expectation rather than a defect in our port: chibi's
    // suite does `(list-queue-append! x …)` and
    // then asserts `x` is unchanged; SRFI 117 says of that procedure "it is
    // an error to assume anything about the contents of the list-queues
    // after the procedure returns", so the assertion tests chibi's own
    // choice rather than the specification. Patina ships the SRFI's
    // reference implementation, which reuses the storage the spec frees it
    // to reuse — and Larceny's suite, which does not make that assumption,
    // passes 40 of 40.
    //
    // 34 → 38 and 109 → 111 when both suites were re-vendored at chibi
    // 32ed54b0 and c00200ec (2026-08-27), fixing the four defects this tree
    // reported as #1179 and #1180 and adding regression tests for each. The
    // SRFI reference implementations Patina ships never had them, so all six
    // assertions passed on arrival. The one 117 failure below is unrelated and
    // survives the refresh — it is the `list-queue-append!` assumption above.
    (srfi_117_list_queue, "srfi 117", "(srfi 117 test)", 1, 38),
    (srfi_127_lseq, "srfi 127", "(srfi 127 test)", 0, 111),
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
    // Verbatim. The suite shadows `(scheme base)`'s `quote`, `list`, `cons`
    // and `car` with SRFI 101's, so every `(chibi test)` macro in it expands
    // where those names mean something else — the sharpest test of
    // referential transparency in this table, and what found triage
    // families 33–35 (2026-08-26).
    (srfi_101_rlist, "srfi 101", "(srfi 101 test)", 0, 56),
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
    // The chibi suites are from the same pinned snowballs as the libraries
    // themselves (test-lib/chibi/PROVENANCE.md) — restored after the
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
    // SRFI 146 ships its own supporting libraries under their authors' names,
    // and they are bundled verbatim beside it rather than renamed. Neither is
    // a public API — nothing outside `(srfi 146)` and `(srfi 146 hash)`
    // imports them, and the two suites registered above exercise them through
    // those, since they *are* the implementations. Upstream ships suites for
    // the Gleckler libraries; they are not vendored, because a HAMT suite
    // that passes while `(srfi 146 hash)` fails would tell us nothing we do
    // not already learn from the 77 rows that matter.
    (
        "nieper",
        "the red-black tree under (srfi 146); exercised by that library's own suite",
    ),
    (
        "gleckler",
        "the HAMT under (srfi 146 hash); exercised by that library's own suite",
    ),
];

/// Shared by the SRFI 159 rows below: upstream ships one suite, for
/// `(srfi 159)` itself, and the registered row above runs it.
const SRFI_159_REASON: &str = "a sub-library of (srfi 159), whose single upstream suite — registered above, 316 assertions — drives all of them; upstream ships no per-library suites";

/// Shared by the SRFI 160 rows below: upstream registers a suite for `s16`
/// only, so the rest are covered by our own file rather than by nothing.
const SRFI_160_REASON: &str = "sed-expanded from the template whose s16 expansion the registered (srfi 160 test) row runs; tests/scheme/srfi/homogeneous-vectors.scm covers the per-type parameters";

/// `(srfi 160 base)`, `(srfi 160 c64)` and `(srfi 160 c128)` need their own
/// reason: the template argument does not reach them at all. `base` is a
/// hand-written library rather than an expansion, and the two complex types
/// are the expansion's only ones with no SRFI 4 layer underneath — they wrap
/// an `f32vector`/`f64vector` instead. Upstream does ship a `base` suite, and
/// it is not vendored: like SRFI 4's, it is print-only, with its own
/// `test-assert`/`test-not` macros displaying "OK" or "FAIL" and reporting
/// nothing a driver can read, so a row for it would be vacuously green.
const SRFI_160_COMPLEX_REASON: &str = "upstream's (srfi 160 base) suite is print-only and never reports, the same failure mode SRFI_64_BODY exists to prevent; tests/scheme/srfi/homogeneous-vectors.scm covers the base layer and the two complex types";

/// Every library Patina provides from a tree not excused above either has its
/// upstream suite in the table above or a recorded reason here for not
/// having one. Before this guard, "add a suite when Patina bundles the
/// library" was a comment.
///
/// "Provides" spans both roots — `lib/`, which is shipped, and `test-lib/`,
/// which the test lanes supply with `-A`. Scoping it to `lib/` would let a
/// library escape the obligation by moving, which is exactly what #196 did
/// with `(chibi filesystem)`.
///
/// Each reason is a claim to re-verify when circumstances change, not a
/// permanent pass — several name the event that retires them.
const NO_SUITE: &[(&str, &str)] = &[
    (
        "srfi 1",
        "upstream suite imports (chibi), chibi's implementation core",
    ),
    ("srfi 8", "no upstream suite exists (receive: one macro)"),
    // The four re-export shims of #390. Each names functionality R7RS-small
    // already provides, so what there is to test is that the shim loads and
    // that a binding reached through it works — which is behavioural, not
    // conformance, and is pinned in reexport_shims.rs beside (srfi 16),
    // (srfi 23) and (srfi 98). Running a SRFI's own suite against the R7RS
    // procedure would be testing (scheme base) under another name.
    (
        "srfi 6",
        "re-export shim over (scheme base)'s string ports, which are R7RS 6.13 verbatim; reexport_shims.rs pins it",
    ),
    (
        "srfi 9",
        "re-export shim over (scheme base)'s define-record-type, which is R7RS 5.5 and a superset of the SRFI's form; reexport_shims.rs pins both shapes",
    ),
    (
        "srfi 11",
        "re-export shim over (scheme base)'s let-values and let*-values, which are R7RS 4.2.2; reexport_shims.rs pins it",
    ),
    (
        "srfi 39",
        "re-export shim over (scheme base)'s parameter objects, which are R7RS 4.2.6; reexport_shims.rs pins that parameterize restores after the body",
    ),
    // SRFI 159's sub-libraries. Upstream ships one suite, for `(srfi 159)`,
    // and it drives all of them: the 316 assertions registered above cover
    // the base combinators, the columnar and pretty-printing layers, the
    // colour escapes and the Unicode width tables. Splitting them into
    // per-library rows would need suites upstream does not have.
    ("srfi 159 base", SRFI_159_REASON),
    ("srfi 159 color", SRFI_159_REASON),
    ("srfi 159 columnar", SRFI_159_REASON),
    ("srfi 159 unicode", SRFI_159_REASON),
    ("srfi 159 internal base", SRFI_159_REASON),
    ("srfi 159 internal compat", SRFI_159_REASON),
    ("srfi 159 internal pretty", SRFI_159_REASON),
    ("srfi 159 internal util", SRFI_159_REASON),
    (
        "srfi 115 boundary",
        "generated Unicode word-boundary tables for (srfi 115), exercised by its suite above; upstream ships no suite for the data alone",
    ),
    // SRFI 4's own suite exists but cannot be registered here: it is a
    // print-only harness that displays "OK" or "FAIL" per assertion and exits
    // 0 either way, with no counter and no status a driver can read, so a row
    // for it would be vacuously green — the failure mode SRFI_64_BODY exists
    // to prevent. tests/scheme/srfi/homogeneous-vectors.scm covers it under a
    // framework that reports.
    (
        "srfi 4",
        "upstream's suite prints results and never reports them; tests/scheme/srfi/homogeneous-vectors.scm covers it",
    ),
    // The nine per-type SRFI 160 libraries upstream's suite does not reach.
    // It tests s16 alone — registered above — on its own stated grounds, that
    // the twelve are sed-expanded from one template so a template defect
    // shows in all of them. What that argument does not cover is the per-type
    // parameters the expansion substitutes, which is what
    // tests/scheme/srfi/homogeneous-vectors.scm exercises, importing each of
    // these nine.
    ("srfi 160 u8", SRFI_160_REASON),
    ("srfi 160 s8", SRFI_160_REASON),
    ("srfi 160 u16", SRFI_160_REASON),
    ("srfi 160 u32", SRFI_160_REASON),
    ("srfi 160 s32", SRFI_160_REASON),
    ("srfi 160 u64", SRFI_160_REASON),
    ("srfi 160 s64", SRFI_160_REASON),
    ("srfi 160 f32", SRFI_160_REASON),
    ("srfi 160 f64", SRFI_160_REASON),
    // And the three the template argument does not reach: see the constant.
    ("srfi 160 base", SRFI_160_COMPLEX_REASON),
    ("srfi 160 c64", SRFI_160_COMPLEX_REASON),
    ("srfi 160 c128", SRFI_160_COMPLEX_REASON),
    // The three shims (srfi 146) needed. Each is one macro or a re-export,
    // and each is exercised by the two SRFI 146 suites above, which do not
    // load without them.
    (
        "srfi 2",
        "and-let*: one macro, no portable upstream suite (the SRFI predates syntax-rules); tests/scheme/srfi/and-let.scm checks each clause form against the SRFI's text",
    ),
    (
        "srfi 16",
        "re-export shim over (scheme case-lambda), which is already SRFI 16's own reference implementation; reexport_shims.rs pins it",
    ),
    (
        "srfi 145",
        "assume: one macro, and the SRFI ships no suite; tests/scheme/srfi/and-let.scm covers it beside SRFI 2",
    ),
    (
        "srfi 23",
        "re-export shim over (scheme base)'s error; reexport_shims.rs pins it",
    ),
    (
        "srfi 124",
        "the SRFI ships implementations but no tests, and chibi's (srfi 124) has none either; Larceny's `ephemeron` suite is the only upstream one and is a lane, not a cargo test. crates/patina-tests/tests/ephemerons.rs pins the behaviour on both backends, forcing collection directly rather than by allocating 100 million pairs",
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
        "rename shim over (srfi 142), and through it over (srfi 151), whose suite runs above; tests/scheme/srfi/bitwise.scm pins the renames. chibi's own (srfi 33) suite is no substitute: its two bitwise-merge assertions expect SRFI 151's argument order, which contradicts SRFI 33's text and chibi's own implementation",
    ),
    (
        "srfi 60",
        "rename shim over (srfi 151), whose suite runs above; tests/scheme/srfi/bitwise.scm pins the MSB-first deviations",
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
        "rename shim over (srfi 151), whose suite runs above; tests/scheme/srfi/bitwise.scm pins the bitwise-if swap",
    ),
    (
        "chibi filesystem",
        "upstream suite opens a raw file descriptor before its directory tests, hitting the FFI stub outside any test form, which aborts the run; add the suite when FFI lands",
    ),
    (
        "srfi 64",
        "its upstream suite is a *program* (compat/vendor/srfi-64/test.scm, 259 lines testing SRFI 64 through SRFI 64's own runner API), not a `(… test)` library, so it does not fit suite_tests!'s shape. It runs today as the srfi-64 corpus package; the next build_corpus.py run drops that package because we now bundle the library, and #193's driver is the intended new home. This entry retires when it lands there",
    ),
    (
        "chibi test",
        "the framework itself — exercised by every suite above, the self-check below, and the chibi R7RS gate",
    ),
];

#[test]
fn every_provided_library_has_a_suite_or_a_recorded_reason() {
    let mut all = common::shipped_libraries(&repo_root().join("lib"));
    let supplied = common::shipped_libraries(&common::test_lib_root());
    assert!(
        !supplied.is_empty(),
        "found no .sld files under test-lib/ — wrong root?"
    );
    all.extend(supplied);
    assert!(
        !all.is_empty(),
        "found no provided .sld files — wrong root?"
    );

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
            "({lib}) is provided but its upstream suite does not run: add a suite_tests! \
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
            "NO_SUITE names ({lib}), which Patina does not provide — delete the entry"
        );
    }
    for tree in &excused_trees {
        assert!(
            all.iter().any(|name| name[0] == *tree),
            "NO_SUITE_TREES names ({tree}), which holds no provided libraries — delete the entry"
        );
    }
}

/// The same proof for the SRFI 64 path, which has its own counters and so its
/// own way of being vacuously green.
///
/// `SRFI_64_BODY`'s documented failure mode is reporting zero failures and
/// zero assertions — a mistyped accessor, or `(chibi test)` winning the
/// import-order race for the five macro names both frameworks export, would
/// do it silently. So this runs a suite with one of each outcome and checks
/// the two numbers exactly.
///
/// The `xpass` row is the one that matters: it is a `test-expect-fail` that
/// passed, and it must land in `failures`. Reverting that term makes this
/// test report `(1, 5)` against the expected `(2, 5)` — checked, not assumed.
///
/// Six rows: 2 pass, 1 fail, 1 xfail, 1 xpass, 1 skip. Five executed
/// (the skip is excluded, as `scheme_suite.rs` excludes it), and two count as
/// failures (the plain fail and the xpass).
#[test]
fn srfi_64_body_reports_failures_and_counts() {
    fn expect<B: Backend>(interp: &Interpreter<B>, label: &str) {
        let suite = "(define (run-tests) \
                       (test-begin \"srfi-64 self-check\") \
                       (test-equal 1 1) \
                       (test-equal 2 2) \
                       (test-equal 3 4) \
                       (test-expect-fail 1) (test-equal 5 6) \
                       (test-expect-fail 1) (test-equal 7 7) \
                       (test-skip 1) (test-equal 8 9) \
                       (test-end))";
        let program = harness_program("(srfi 64)", &format!("{suite} {SRFI_64_BODY}"));
        assert_eq!(
            counts_on(interp, label, &program),
            (2, 5),
            "the SRFI 64 path must count the xpass as a failure and the skip \
             as not run, or the two SRFI 146 rows' numbers mean nothing"
        );
    }

    let tw = common::tree_walker_interpreter();
    expect(&tw, "srfi-64 self-check on tree-walker");

    let vm = common::vm_interpreter();
    expect(&vm, "srfi-64 self-check on vm");
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

    // `harness_program` imports `(chibi test)`, so these need `test-lib/` on
    // the path like every other interpreter in this file.
    let tw = common::tree_walker_interpreter();
    expect_one_failure_of_three(&tw, "self-check on tree-walker");

    let vm = common::vm_interpreter();
    expect_one_failure_of_three(&vm, "self-check on vm");
}

/// `(scheme show)` and `(srfi 159)` must resolve with **only `lib/` on the
/// search path**.
///
/// Worth its own row for the same reason `(srfi 130)`'s is below: the suite
/// row above runs under `check_suite`, which supplies `test-lib/`, so it
/// would pass unchanged if any link in this chain leaked into a supplied
/// root. SRFI 159's chain is the longest of any bundled library here — eight
/// libraries of its own over `(srfi 1)`, `(srfi 69)`, `(srfi 117)`,
/// `(srfi 130)` and `(srfi 151)` — and `internal/base.sld` picks its hash
/// tables with `(library (srfi 69))`, so a shipping tree missing that one
/// would silently take the `(srfi 125)` branch and fail on SRFI 69's calling
/// convention rather than on the import.
#[test]
fn srfi_159_chain_resolves_from_the_shipped_tree_alone() {
    use common::eval_program_shipped_only as shipped;
    assert_eq!(
        shipped("(import (scheme base) (srfi 159)) (show #f (numeric/comma 1234567))"),
        "\"1,234,567\""
    );
    // The alias, and the columnar layer under it — the deepest sub-library,
    // and the one whose imports reach furthest outside the tree, to
    // `(srfi 117)` and `(srfi 130)`. Its width comes from the `width`
    // parameter's default rather than the terminal, so asserting the padded
    // column is stable; what this pins is that the chain loads at all.
    assert_eq!(
        shipped(
            "(import (scheme base) (scheme show)) \
             (show #f (columnar (each \"ab\" nl) (each \"1\" nl)))"
        ),
        "\"ab                                     1\\n\""
    );
}

// ─── Where (srfi 130) resolves from ──────────────────────────────────────────
//
// Moved here from `srfi_130_string.rs` when #193 took its portable rows to
// `tests/scheme/srfi/string-cursors.scm`. These two cannot be suite rows: the
// driver supplies `test-lib/` like every shared helper, and so does
// `check_suite` above, which is why they sit beside it. The `srfi_130_string`
// suite row would pass unchanged if the chain below leaked into `test-lib/`.

/// Importing `(srfi 130)` alone must pull in `(srfi 14)` from the bundled
/// tree, with **only `lib/` on the search path**.
///
/// #198 shortened the chain. The subset of `(chibi string)` that `(srfi 130)`
/// used is inlined into `130.chibi-string.scm`, so `lib/chibi/` is gone and
/// the only bundled dependency left is `(srfi 14)` — for the two char-set
/// names the inlined `make-char-predicate` uses. The companion test below
/// pins the other half: the library itself is *supplied*, not bundled.
#[test]
fn srfi_130_chain_resolves_from_the_shipped_tree_alone() {
    use common::eval_program_shipped_only as shipped;
    assert_eq!(
        shipped("(import (scheme base) (scheme char) (srfi 130)) (string-null? \"\")"),
        "#t"
    );
    assert_eq!(
        shipped("(import (scheme base) (srfi 14)) (char-set-contains? char-set:digit #\\7)"),
        "#t"
    );
}

/// `(chibi string)` moved to `test-lib/` in #198 rather than being deleted:
/// 35 corpus packages import it, and deleting it cost nine of them. So it is
/// supplied like the rest of `test-lib/chibi/` — unreachable from `lib/`
/// alone, reachable with the root. Both halves matter: the first is why
/// `(srfi 130)` had to stop importing it, the second is why the corpus tally
/// did not move.
#[test]
fn chibi_string_is_supplied_not_bundled() {
    let err = common::eval_program_shipped_only_err(
        r#"(import (scheme base) (chibi string)) (string-count "aab" #\a)"#,
    );
    assert!(
        err.contains("chibi string"),
        "(chibi string) resolved from lib/ alone — it is supplied from test-lib/: {err}"
    );
    // And it does resolve once the root is supplied, which every shared helper does.
    assert_eq!(
        common::eval_program(r#"(import (scheme base) (chibi string)) (string-count "aab" #\a)"#),
        "2"
    );
}
