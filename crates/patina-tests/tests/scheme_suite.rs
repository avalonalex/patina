//! The driver for Scheme-language test files (#193).
//!
//! `tests/scheme/**/*.scm` are ordinary, portable SRFI 64 programs. This file
//! runs each of them on every backend and holds it to an expectation. Adding a
//! backend touches this driver; adding a test touches neither.
//!
//! # The files are plain programs, not a harness format
//!
//! A test file is exactly what you would write by hand:
//!
//! ```scheme
//! (import (scheme base) (srfi 64))
//! (test-begin "call-with-values consumers")
//! (test-equal 3 (call-with-values (lambda () (values 1 2)) +))
//! (test-end)
//! ```
//!
//! Run it directly and it behaves like any SRFI 64 program —
//! `./target/release/patina tests/scheme/foo.scm` prints a summary. Run it
//! under chibi or Gauche and it does the same, which is the property that
//! makes these files an oracle rather than only a suite (#193 Phase 3).
//!
//! # How the driver reads the result, and why not the obvious way
//!
//! Not from the exit status. Measured on SRFI 64's own runner: **`test-end`
//! never signals** — not for a plain failure, not for an unexpected pass. Only
//! `test-exit` exits non-zero, and this driver runs *in-process*, where
//! `(exit 1)` would take the test binary down with it. A driver that ran a
//! file and checked its status would report green on a suite that failed
//! every assertion, which is the shape audit E1 already found once.
//!
//! So the driver reads the runner's own counts, which survive `test-end`:
//! `fail` and `xpass` must both be zero. `xpass` is the load-bearing one — it
//! is what makes a quarantined divergence *fail* once its bug is fixed, so a
//! `test-expect-fail` that starts passing breaks the build instead of quietly
//! going green. `harness_reports_each_result_kind` below pins all of it.
//!
//! # Two things the driver imposes on the file
//!
//! Before the file runs, the driver installs `(test-runner-null)` as the
//! current runner. That suppresses SRFI 64's banner and summary — this is a
//! cargo test, not a report — and, more importantly, stops it writing a
//! `<suite>.log` into the working directory, which for an in-process driver is
//! the crate root. Counts are still accumulated.
//!
//! After the file runs, the driver appends an expression that reads those
//! counts back. Neither is visible to the file, which is why the file stays
//! portable.
//!
//! # A sharp edge worth knowing before you hit it
//!
//! An assertion whose expression raises a **non-error object** aborts the rest
//! of the file rather than recording one failure: SRFI 64's `false-if-error`
//! calls `error-object-message` on whatever was raised, which is itself a type
//! error. `(test-equal 1 (raise 'x))` stops the file at that line, and the
//! driver then reports it as "failed to run" rather than as one bad row.
//!
//! Wrap such a raise in `guard` and assert on what the guard produces — which
//! is what every row in `callability.scm` does, for reasons of its own. This
//! is upstream SRFI 64 behaviour, not something the driver can paper over.

mod common;
use common::repo_root;
use patina_interpreter::Interpreter;
use patina_runtime::Backend;
use std::path::{Path, PathBuf};

/// Every `.scm` file the driver runs, with the **minimum** number of
/// assertions it must report.
///
/// The floor is not bookkeeping. A file that stops running — a typo in an
/// import, a `test-begin` whose group is skipped, a truncated file — reports
/// zero failures and would otherwise pass. This is the same guard
/// `run_chibi_tests.sh` gets from pinning `EXPECTED_TOTAL`, per file, and #193
/// asks for it from the first commit rather than after the first silent skip.
///
/// A minimum rather than an exact count so that adding an assertion to a file
/// does not require editing Rust; lowering one still does.
const SUITE: &[(&str, i64)] = &[
    ("control/callability.scm", 26),
    ("control/case-lambda.scm", 20),
    ("control/internal-escape-boundaries.scm", 11),
    ("control/parameters.scm", 18),
    ("control/tail-recursion.scm", 36),
    ("control/wind-thunk-exceptions.scm", 12),
];

fn scheme_dir() -> PathBuf {
    repo_root().join("crates/patina-tests/tests/scheme")
}

/// What one file reported on one backend.
#[derive(Debug, PartialEq, Eq)]
struct Counts {
    pass: i64,
    fail: i64,
    xpass: i64,
    xfail: i64,
    skip: i64,
}

impl Counts {
    /// Assertions that actually *executed*. Skips are excluded deliberately:
    /// counting them was a hole exactly the shape of the one the floor exists
    /// to close — `(test-skip 100)` turns every row into a skip, so a file that
    /// executed nothing still cleared a floor of 34. The floor now measures
    /// what ran, and `skip` is asserted to be zero separately, so a file cannot
    /// quietly stop testing by skipping instead of by breaking.
    fn ran(&self) -> i64 {
        self.pass + self.fail + self.xpass + self.xfail
    }
}

/// Quiet the runner, run `program`, and read the counts back.
///
/// Three evaluations on one interpreter rather than one spliced program: the
/// file has its own `(import …)` at the top, and an import is not a form that
/// can be nested inside a wrapper. Sharing the interpreter is what lets the
/// driver set up before and inspect after while the file stays a plain
/// top-level program.
fn run_on<B: Backend>(interp: &Interpreter<B>, label: &str, program: &str) -> Counts {
    interp
        .eval_program("(import (scheme base) (srfi 64)) (test-runner-current (test-runner-null))")
        .unwrap_or_else(|e| panic!("[{label}] could not install the null runner: {e}"));

    interp
        .eval_program(program)
        .unwrap_or_else(|e| panic!("[{label}] failed to run: {e}"));

    let value = interp
        .eval_program(
            "(let ((r (test-runner-current)))
               (list (test-runner-pass-count r) (test-runner-fail-count r)
                     (test-runner-xpass-count r) (test-runner-xfail-count r)
                     (test-runner-skip-count r)))",
        )
        .unwrap_or_else(|e| panic!("[{label}] could not read the runner counts: {e}"));

    let text = patina_primitives::primitives::io::datum_writer::format_display_tagged(
        value,
        interp.backend().global_env().heap(),
    );
    let nums: Vec<i64> = text
        .trim_matches(|c| c == '(' || c == ')')
        .split_whitespace()
        .map(|n| {
            n.parse()
                .unwrap_or_else(|_| panic!("[{label}] expected five integers, got {text:?}"))
        })
        .collect();
    assert_eq!(
        nums.len(),
        5,
        "[{label}] expected five counts, got {text:?}"
    );
    Counts {
        pass: nums[0],
        fail: nums[1],
        xpass: nums[2],
        xfail: nums[3],
        skip: nums[4],
    }
}

/// Run `program` on both backends, requiring them to agree.
///
/// Agreement is checked on the whole count vector, not just on "did it pass".
/// Two backends can reach zero failures having run different numbers of
/// assertions — a `cond-expand` that skips a group on one of them would do
/// exactly that — and that divergence is the kind this suite exists to expose.
fn run_on_both_backends(label: &str, program: &str) -> Counts {
    let tw = run_on(
        &common::tree_walker_interpreter(),
        &format!("{label} (tree-walker)"),
        program,
    );
    let vm = run_on(&common::vm_interpreter(), &format!("{label} (vm)"), program);
    assert_eq!(
        tw, vm,
        "[{label}] the backends disagree on what ran: tree-walker {tw:?}, vm {vm:?}"
    );
    vm
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn every_scheme_file_passes_on_both_backends() {
    // Collected, not asserted in the loop: a panic on the first file would hide
    // every later one, and the point of a suite is to learn what *all* of it
    // says in one run. Same reason `cargo test` needs `--no-fail-fast`.
    let mut problems: Vec<String> = Vec::new();

    for (name, floor) in SUITE {
        let path = scheme_dir().join(name);
        let counts = run_on_both_backends(name, &read(&path));
        // SRFI 64 names its log after the *suite*, not the path, and writes it
        // to the cwd — so the repro below says `<basename>.log`, not
        // `control/<name>.log`, which would not exist.
        let stem = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name);

        if counts.fail != 0 {
            problems.push(format!(
                "[{name}] {} assertion(s) failed. To see which, run it from a \
                 scratch directory — SRFI 64 puts per-assertion detail in a log \
                 beside the cwd, not on stdout:\n  \
                 (cd $(mktemp -d) && $OLDPWD/target/release/patina -A $OLDPWD/test-lib \
                 $OLDPWD/crates/patina-tests/tests/scheme/{name} && cat {stem}.log)",
                counts.fail
            ));
        }
        if counts.xpass != 0 {
            problems.push(format!(
                "[{name}] {} test(s) marked `test-expect-fail` now pass. That is \
                 the quarantine doing its job: delete the expectation, and the \
                 row it guarded becomes an ordinary assertion.",
                counts.xpass
            ));
        }
        if counts.skip != 0 {
            problems.push(format!(
                "[{name}] {} test(s) skipped. Nothing here should skip: a skipped \
                 row asserts nothing while still looking like a row. If a row \
                 genuinely cannot run on a backend, mark it `test-expect-fail` \
                 so it is visible and retires itself.",
                counts.skip
            ));
        }
        if counts.ran() < *floor {
            problems.push(format!(
                "[{name}] ran {} assertions, expected at least {floor} — a file \
                 that stops running reports no failures, so the floor is what \
                 tells the difference between passing and not happening. \
                 Counts: {counts:?}",
                counts.ran()
            ));
        }
    }

    assert!(problems.is_empty(), "\n{}", problems.join("\n\n"));
}

/// Every file declares its own imports, sufficient to reach its first
/// `test-begin`.
///
/// The driver installs `(srfi 64)` into the same environment the file then
/// runs in — it has to, because the null runner must exist before the file's
/// `test-begin`, and the runner has to survive to the count-read afterwards,
/// which means one interpreter. The cost is that a file which *omits or
/// misspells its own import* would still pass here while failing standalone,
/// and standalone is the whole oracle property: these files are supposed to
/// run under `patina`, chibi and Gauche unchanged.
///
/// So the prelude — everything up to the first `(test-begin` — is evaluated in
/// a *fresh* interpreter with nothing pre-imported. That is enough to catch a
/// missing or wrong import, and stops short of running any assertion, so no
/// SRFI 64 log file is written into the crate root.
#[test]
fn every_file_carries_its_own_imports() {
    for (name, _) in SUITE {
        let text = read(&scheme_dir().join(name));
        let cut = text.find("(test-begin").unwrap_or_else(|| {
            panic!("[{name}] has no `(test-begin` — every file is an SRFI 64 program")
        });
        let prelude = &text[..cut];
        assert!(
            prelude.contains("(import "),
            "[{name}] declares no imports before its first `(test-begin`"
        );
        for (backend, result) in [
            (
                "tree-walker",
                common::tree_walker_interpreter()
                    .eval_program(prelude)
                    .err()
                    .map(|e| e.to_string()),
            ),
            (
                "vm",
                common::vm_interpreter()
                    .eval_program(prelude)
                    .err()
                    .map(|e| e.to_string()),
            ),
        ] {
            assert!(
                result.is_none(),
                "[{name}] its own prelude does not evaluate on {backend}, so the file \
                 depends on something the driver happens to import for it and would \
                 fail standalone: {}",
                result.unwrap()
            );
        }
    }
}

/// Every `.scm` file on disk is in [`SUITE`], and vice versa.
///
/// Without this a file can be added and never run — passing by absence, which
/// is the failure the floors above exist to prevent, one level up.
#[test]
fn the_suite_table_and_the_directory_agree() {
    let dir = scheme_dir();
    // Joined with `/` from components rather than `to_string_lossy()`: the
    // relative path has more than one component now, and a platform separator
    // would not match SUITE's literals. `common::shipped_libraries` exists
    // because three tests had already grown three copies of this mistake.
    let mut on_disk: Vec<String> = common::files_under(&dir)
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("scm"))
        .map(|p| {
            p.strip_prefix(&dir)
                .expect("under tests/scheme")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    on_disk.sort();
    let mut listed: Vec<String> = SUITE.iter().map(|(n, _)| (*n).to_string()).collect();
    listed.sort();
    assert_eq!(
        on_disk, listed,
        "tests/scheme/ and the SUITE table disagree — a file was added or removed \
         without a row, so it would run nowhere or be looked for in vain"
    );
}

/// The driver's own instrument check: it must report each result kind, and in
/// particular must not treat an unexpected pass as success.
///
/// This is #193's Phase 0 acceptance criterion — "an xfail that flips to xpass
/// must fail the run" — as a test rather than a claim. It is deliberately not
/// a `.scm` file in `tests/scheme/`, because it must *fail* the assertions the
/// real files are held to.
#[test]
fn harness_reports_each_result_kind() {
    let counts = run_on_both_backends(
        "self-check",
        r#"(import (scheme base) (srfi 64))
           (test-begin "deliberate")
           (test-equal 1 1)              ; pass
           (test-equal 1 2)              ; fail
           (test-expect-fail 1)
           (test-equal 'broken 'broken)  ; xpass — the bug is fixed
           (test-expect-fail 1)
           (test-equal 3 4)              ; xfail — still broken, as expected
           (test-end)"#,
    );
    assert_eq!(
        counts,
        Counts {
            pass: 1,
            fail: 1,
            xpass: 1,
            xfail: 1,
            skip: 0
        },
        "the driver cannot tell the four result kinds apart, so every \
         expectation above it is unfounded"
    );
}

/// `test-end` returning normally is exactly why the driver reads counts.
///
/// If this ever starts failing, SRFI 64 has gained an error-signalling
/// `test-end` and the driver could be simplified — but until then, a driver
/// that trusted control flow would see nothing wrong with a failing file.
#[test]
fn test_end_does_not_signal_a_failure() {
    let interp = common::vm_interpreter();
    interp
        .eval_program("(import (scheme base) (srfi 64)) (test-runner-current (test-runner-null))")
        .expect("install runner");
    interp
        .eval_program("(test-begin \"quiet-failure\") (test-equal 1 2) (test-end)")
        .expect(
            "test-end returned an error — SRFI 64 changed, and the driver's \
             count-reading may now be redundant",
        );
}
