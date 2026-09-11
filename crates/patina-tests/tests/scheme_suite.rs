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
//! **That property is checked in CI, not here.** This driver runs only the two
//! Patina backends. The oracles are `scripts/run_suite_oracles.sh`, which runs
//! every file under chibi and Gauche and holds each oracle's differing rows to
//! `DIVERGENCES.tsv` — the CI job "Suite oracles" runs it on every PR against
//! pinned oracle versions (#193 Phase 3), so a row edited into a new
//! disagreement fails there, classified or not. What CI does *not* check is the
//! prose: several files' headers carry a pass/fail tally measured on a stated
//! date, and the rule is still that a PR touching a file's rows re-measures its
//! header.
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
use common::{files_under, repo_root};
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
    ("control/callability.scm", 30),
    ("control/case-lambda.scm", 20),
    ("control/cps-features.scm", 101),
    ("control/guard.scm", 15),
    ("control/internal-escape-boundaries.scm", 11),
    ("control/parameters.scm", 18),
    ("control/prompts.scm", 60),
    ("control/tail-recursion.scm", 36),
    ("control/values.scm", 5),
    ("control/wind-thunk-exceptions.scm", 14),
    ("data/circular-data.scm", 36),
    ("data/case-mapping.scm", 2),
    ("data/conversion.scm", 62),
    ("data/external-representation.scm", 12),
    ("data/lists.scm", 77),
    ("data/numeric-operations.scm", 47),
    ("data/numeric-predicates.scm", 92),
    ("data/predicates.scm", 62),
    ("data/record-types.scm", 23),
    ("data/strings.scm", 149),
    ("data/vectors.scm", 79),
    ("expansion/define-values.scm", 14),
    ("expansion/ellipsis.scm", 7),
    ("expansion/ellipsis-containers.scm", 13),
    ("expansion/hygiene.scm", 38),
    ("expansion/keyword-bindings.scm", 14),
    ("expansion/let-syntax.scm", 31),
    ("expansion/let-values.scm", 1),
    ("expansion/quasiquote.scm", 1),
    ("expansion/syntax-rules-literals.scm", 19),
    ("expansion/template-references.scm", 6),
    ("reader/at-identifiers.scm", 11),
    ("reader/line-endings.scm", 4),
    ("reader/unicode-identifiers.scm", 20),
    ("reader/vertical-bar-identifiers.scm", 32),
    ("srfi/bitwise.scm", 81),
    ("srfi/fixnums.scm", 54),
    ("srfi/sorting.scm", 5),
    ("srfi/string-cursors.scm", 15),
    ("stdlib/comparators.scm", 5),
    ("stdlib/eval.scm", 19),
    ("stdlib/hash-tables.scm", 6),
    ("stdlib/lazy-evaluation.scm", 32),
    ("stdlib/list.scm", 6),
    ("stdlib/ports.scm", 8),
    ("stdlib/process-context.scm", 12),
    ("stdlib/random.scm", 4),
    ("stdlib/scheme-r5rs.scm", 20),
    ("stdlib/time.scm", 5),
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
fn run_on<B: Backend>(
    interp: &Interpreter<B>,
    label: &str,
    program: &str,
) -> Result<Counts, String> {
    interp
        .eval_program("(import (scheme base) (srfi 64)) (test-runner-current (test-runner-null))")
        .map_err(|e| format!("[{label}] could not install the null runner: {e}"))?;

    interp
        .eval_program(program)
        .map_err(|e| format!("[{label}] failed to run: {e}"))?;

    // Each count is read on its own, in an expression that constructs nothing.
    //
    // The obvious version builds one list — `(list (test-runner-pass-count r)
    // …)` — and parses it. That is fragile in a way only one kind of file
    // reveals: the driver evaluates this in the *same* interpreter the file
    // just ran in, so whatever the file imported is still in scope.
    // `expansion/quasiquote.scm` imports SRFI 101, whose `list` builds
    // random-access lists, and the count expression came back as
    // `#<record kons>`. A file is entitled to rebind core names — that one
    // exists precisely to check that quasiquote survives it — so the driver
    // must not depend on any of them.
    //
    // Five evaluations rather than one, each returning a bare integer. What
    // that buys precisely: the read no longer depends on a *constructor*,
    // which is the kind of name a file is most likely to rebind — SRFI 101
    // exports `list`, `append` and `quote`, and a numeric library could
    // plausibly export others. It is not immunity. SRFI 64's own accessors
    // are ordinary identifiers too, and a file that shadowed
    // `test-runner-pass-count` would still fool this; nothing in the suite
    // does, and a library that exported those names would be a strange one.
    //
    // The runner is bound once, into a name the driver chooses, so the five
    // reads are five fields of one object rather than five independent
    // lookups of a parameter the file has just had a chance to disturb.
    interp
        .eval_program("(define %suite-runner (test-runner-current))")
        .map_err(|e| format!("[{label}] could not capture the runner: {e}"))?;

    let mut nums = [0i64; 5];
    for (slot, accessor) in nums.iter_mut().zip([
        "test-runner-pass-count",
        "test-runner-fail-count",
        "test-runner-xpass-count",
        "test-runner-xfail-count",
        "test-runner-skip-count",
    ]) {
        let value = interp
            .eval_program(&format!("({accessor} %suite-runner)"))
            .map_err(|e| format!("[{label}] could not read {accessor}: {e}"))?;
        let text = patina_primitives::primitives::io::datum_writer::format_display_tagged(
            value,
            interp.backend().global_env().heap(),
        );
        *slot = text
            .trim()
            .parse()
            .map_err(|_| format!("[{label}] {accessor} answered {text:?}, not a count"))?;
    }
    Ok(Counts {
        pass: nums[0],
        fail: nums[1],
        xpass: nums[2],
        xfail: nums[3],
        skip: nums[4],
    })
}

/// Run `program` on both backends. What the two runs must have in common is
/// [`backends_ran_the_same_rows`]'s to say; this only runs them.
///
/// Returns rather than panics so that one file which fails to *run at all* —
/// a resource limit, an error escaping to the top level — costs its own row and
/// not every file after it. `every_scheme_file_passes_on_both_backends` collects
/// these for the same reason it collects assertion failures.
///
/// **An abort is the exception, and it is not one a `Result` can carry.** A
/// stack overflow or a SIGSEGV takes the test process down: no `Err` is built,
/// no counts are read, and every other file in [`SUITE`] loses its report too.
/// That is not hypothetical — `data/circular-data.scm`'s three stack-depth rows
/// exist because the writer used to overflow on a 100_000-element list, so the
/// suite now contains the shape. It is the isolation a migration gives up:
/// those rows used to have a binary of their own, and a regression killed only
/// that one.
fn run_on_both_backends(label: &str, program: &str) -> Result<(Counts, Counts), String> {
    let tw = run_on(
        &common::tree_walker_interpreter(),
        &format!("{label} (tree-walker)"),
        program,
    )?;
    let vm = run_on(&common::vm_interpreter(), &format!("{label} (vm)"), program)?;
    Ok((tw, vm))
}

/// Run `program` on both backends and require identical counts — for the
/// driver's own probes, which declare no backend-scoped expectation.
fn run_identically(label: &str, program: &str) -> Result<Counts, String> {
    let (tw, vm) = run_on_both_backends(label, program)?;
    if tw != vm {
        return Err(format!(
            "[{label}] the backends disagree: tree-walker {tw:?}, vm {vm:?}"
        ));
    }
    Ok(vm)
}

/// The two backends ran the same rows, and expected to fail exactly the rows
/// the file says they would.
///
/// Agreement used to mean identical count vectors, and that is what
/// guarantees a suite file asks both backends the same question: a
/// `cond-expand` that skips a group on one of them reaches zero failures
/// having run fewer assertions, and that is the kind of divergence this suite
/// exists to expose. A **known** divergence between the backends breaks
/// identical counts by construction — the row passes on one and is an
/// expected failure on the other — so the rule is now the same in every
/// respect but one: the same number of rows ran, the same number failed, were
/// unexpectedly passed, and were skipped, and the *expected* failures may
/// differ by exactly what the file declares with backend-scoped
/// `test-expect-fail` lines ([`declared_backend_expectations`]). A file that
/// declares none is held to identical counts, as before.
///
/// That is a stronger guard than a per-file opt-out would be. The difference
/// is pinned to the text, so a backend that quietly starts expecting to fail a
/// row the file never scoped is a mismatch, and so is a scoped line whose row
/// both backends pass — which is how a quarantine retires: the `xpass` check
/// says to delete the line, and this one says the file still declares it.
///
/// Only rows-ran and the `xfail` delta are compared here. `fail`, `xpass` and
/// `skip` are asserted zero per backend by the caller before this runs, so
/// comparing them again would add nothing — and would fire *first* on a
/// retiring quarantine, hiding both messages above behind "the backends
/// disagree". `harness_holds_a_backend_scoped_expectation_to_its_declaration`
/// pins the accepted and the rejected shape.
fn backends_ran_the_same_rows(
    label: &str,
    program: &str,
    tw: &Counts,
    vm: &Counts,
) -> Result<(), String> {
    if tw.ran() != vm.ran() {
        return Err(format!(
            "[{label}] the backends disagree on what ran: tree-walker {tw:?}, vm {vm:?}. \
             Only the split between `pass` and `xfail` may differ between them, \
             and only by what the file declares with backend-scoped \
             `test-expect-fail` lines."
        ));
    }
    let (declared_tw, declared_vm) = declared_backend_expectations(program);
    if tw.xfail - vm.xfail != declared_tw - declared_vm {
        return Err(format!(
            "[{label}] the file declares {declared_tw} tree-walker and {declared_vm} vm \
             backend-scoped expectations, so the tree-walker should record {} more \
             expected failures than the vm, but it recorded {}: tree-walker {tw:?}, \
             vm {vm:?}. A `(cond-expand (patina-<backend> (test-expect-fail n)) (else))` \
             line must sit directly above the row it guards, and must go when the \
             row converges.",
            declared_tw - declared_vm,
            tw.xfail - vm.xfail
        ));
    }
    Ok(())
}

/// The backend-scoped expectations a file declares, as (tree-walker, vm).
///
/// A divergence between the two backends is written as a `test-expect-fail`
/// inside a `cond-expand` clause naming the backend known to get the row
/// wrong:
///
/// ```scheme
/// (cond-expand (patina-tree-walker (test-expect-fail 1)) (else))
/// (test-equal "the row's name" <the right answer> <the program>)
/// ```
///
/// `patina-vm` and `patina-tree-walker` are the feature identifiers each
/// backend advertises at construction (`Heap::add_feature`), so the file asks
/// which backend it is on rather than the harness carrying it — which is what
/// lets the same file run unchanged under chibi and Gauche, where neither
/// identifier is true and the row is an ordinary assertion. The row asserts
/// the *right* answer everywhere; the line says who is known to miss it.
///
/// The scan is [`specifiers_in`]'s — the same one the text check runs — so a
/// specifier that check accepts is one this counts, and vice versa. A
/// specifier under `(patina …)`, or with no clause at all, applies to both
/// backends alike and contributes to neither side.
fn declared_backend_expectations(program: &str) -> (i64, i64) {
    let (mut tw, mut vm) = (0, 0);
    for line in program.lines() {
        let code = code_before_comment(line);
        for (form, spec, at) in specifiers_in(code) {
            if form != "test-expect-fail" {
                continue;
            }
            // The text check requires a positive count and reports a bad one
            // by line; a specifier it would reject counts for nothing here
            // rather than panicking, so one bad line costs that file's report
            // and not every file's.
            let n: i64 = spec.parse().unwrap_or(0);
            // The clause the specifier sits in opens at the last `(` before it,
            // and the feature name is what follows that paren.
            let clause = code[..at].rfind('(').map(|open| code[open + 1..at].trim());
            match clause {
                Some("patina-tree-walker") => tw += n,
                Some("patina-vm") => vm += n,
                _ => {}
            }
        }
    }
    (tw, vm)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

#[test]
fn every_scheme_file_passes_on_both_backends() {
    // Collected, not asserted in the loop: a panic on the first file would hide
    // every later one, and the point of a suite is to learn what *all* of it
    // says in one run. Same reason `cargo test` needs `--no-fail-fast`.
    let mut problems: Vec<String> = Vec::new();

    for (name, floor) in SUITE {
        let text = match read(&scheme_dir().join(name)) {
            Ok(text) => text,
            Err(problem) => {
                problems.push(problem);
                continue;
            }
        };
        let (tw, vm) = match run_on_both_backends(name, &text) {
            Ok(counts) => counts,
            Err(problem) => {
                problems.push(problem);
                continue;
            }
        };
        // SRFI 64 names its log after the *suite*, not the path, and writes it
        // to the cwd — so the repro below says `<basename>.log`, not
        // `control/<name>.log`, which would not exist.
        let stem = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name);

        // Each backend is held to the file on its own, before the two are
        // held to each other: an `xpass` on one backend is a quarantine
        // retiring, and that message — not "the backends disagree" — is the
        // one the person deleting the line needs to see.
        // The reproduction flag travels with the counts rather than being read
        // back off the label, so rewording a label cannot send someone to
        // reproduce a tree-walker failure on the VM.
        let per_backend: Vec<(&str, &str, &Counts)> = if tw == vm {
            vec![("both backends", "", &vm)]
        } else {
            vec![
                ("the tree-walker", "--tree-walker", &tw),
                ("the vm", "", &vm),
            ]
        };
        for (backend, flag, counts) in per_backend {
            if counts.fail != 0 {
                problems.push(format!(
                    "[{name}] {} assertion(s) failed on {backend}. To see which, run it \
                     from a scratch directory — SRFI 64 puts per-assertion detail in a \
                     log beside the cwd, not on stdout:\n  \
                     (cd $(mktemp -d) && $OLDPWD/target/release/patina {flag} -A $OLDPWD/test-lib \
                     $OLDPWD/crates/patina-tests/tests/scheme/{name} && cat {stem}.log)",
                    counts.fail
                ));
            }
            if counts.xpass != 0 {
                problems.push(format!(
                    "[{name}] {} test(s) marked `test-expect-fail` now pass on {backend}. \
                     That is the quarantine doing its job: delete the expectation — for a \
                     backend-scoped one, the `(cond-expand (patina-… (test-expect-fail …` \
                     line above the row — and the row it guarded becomes an ordinary \
                     assertion. Then update the tracking document the row's comment names.",
                    counts.xpass
                ));
            }
            if counts.skip != 0 {
                problems.push(format!(
                    "[{name}] {} test(s) skipped on {backend}. Nothing here should skip: a \
                     skipped row asserts nothing while still looking like a row. If a row \
                     genuinely cannot run on a backend, mark it `test-expect-fail` \
                     so it is visible and retires itself.",
                    counts.skip
                ));
            }
            if counts.ran() < *floor {
                problems.push(format!(
                    "[{name}] ran {} assertions on {backend}, expected at least {floor} — a \
                     file that stops running reports no failures, so the floor is what \
                     tells the difference between passing and not happening. \
                     Counts: {counts:?}",
                    counts.ran()
                ));
            }
        }
        if let Err(problem) = backends_ran_the_same_rows(name, &text, &tw, &vm) {
            problems.push(problem);
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
        let text = read(&scheme_dir().join(name)).unwrap_or_else(|e| panic!("[{name}] {e}"));
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

/// Every skip and expectation specifier is a positive count, sitting
/// immediately above its row.
///
/// A scoped row is written `(cond-expand (patina) (else (test-skip 1)))` above
/// the row it guards, so that other implementations *report* a skip rather than
/// losing the row silently. The whole construct has one blind spot: Patina takes
/// the `(patina)` branch and never evaluates the `else`, so nothing in a normal
/// run ever looks inside it. Whatever is wrong in there is wrong only on chibi
/// and Gauche, which nobody runs per-PR. Hence a text check.
///
/// A backend-scoped expectation — `(cond-expand (patina-tree-walker
/// (test-expect-fail 1)) (else))`, the spelling of a known divergence between
/// the two backends — is the one clause one of the driver's own lanes *does*
/// evaluate, and [`backends_ran_the_same_rows`] checks its count at run time.
/// The text rules here still apply to it, for the lane that takes the `else`.
///
/// `test-expect-fail` is checked too, and not for symmetry: `lib/srfi/64.scm`
/// routes both through the same `make-pred`, so a name specifier desyncs
/// identically. Its usual shape puts the specifier on the *patina* branch
/// (`data/conversion.scm`), where a desync fails loudly instead of silently —
/// but the mirror shape has the same blind spot as a skip, and the check costs
/// nothing.
///
/// **What it catches, and what it cannot.** Two rules, and they are not equally
/// strong:
///
/// - **The specifier must be a positive count.** SRFI 64 also accepts a *name*
///   and a *predicate*; in this suite the count is required, not merely
///   preferred, and that is what this half enforces. A name keeps a second copy
///   of the row's title that has to stay character-identical to the `test-equal`
///   beneath it — rename the row and the specifier matches nothing, so the row
///   runs on the oracles after all, the guard gone and the file still looking
///   scoped. Zero is barred for a different reason and a sharper one:
///   `(test-skip 0)` is `(test-match-nth 1 0)`, whose predicate is never true,
///   so it type-checks as a count while guarding nothing at all. This half is
///   airtight; the text says which form was written.
/// - **It must sit directly above a test form.** The count binds to the next
///   test the runner *reaches*, so this is a conservative rule against anything
///   drifting into the gap. It is genuinely weaker than it sounds, and the
///   weakness is worth stating rather than discovering: it cannot tell *which*
///   test form follows, so inserting another assertion directly beneath the skip
///   still passes here while moving the skip onto the wrong row. Text cannot
///   know which row was intended. What would is the deeper fix — running each
///   file a second time with the `patina` feature absent, so the `else` branches
///   actually execute and the skip count can be asserted against the number of
///   scoped rows. That needs a mutation path through `Heap`'s feature registry,
///   which is closed on first read by design (see `Heap::add_feature`), so it is
///   its own change and not this one's.
///
/// Line-oriented deliberately: it reads the file the way the person editing it
/// does. The cost is that it only sees a specifier written on one line, which is
/// how every scoped row is written and how the doc shows it.
#[test]
fn every_scoped_row_skips_by_count_and_sits_above_its_row() {
    for (name, _) in SUITE {
        let text = read(&scheme_dir().join(name)).unwrap_or_else(|e| panic!("[{name}] {e}"));
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let code = code_before_comment(line);
            let specs = specifiers_in(code);
            if specs.is_empty() {
                continue;
            }
            for (form, spec, _) in &specs {
                assert!(
                    spec.parse::<u32>().is_ok_and(|n| n > 0),
                    "[{name}:{}] `({form} {spec})` is {}. This suite requires a \
                     positive count, and Patina never evaluates this branch, so \
                     nothing but this check would notice.",
                    i + 1,
                    describe(spec)
                );
            }
            let next = lines[i + 1..]
                .iter()
                .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with(';'));
            let next = next
                .unwrap_or_else(|| panic!("[{name}:{}] a specifier with no row beneath it", i + 1));
            assert!(
                starts_a_test_row(next),
                "[{name}:{}] the count binds to the next test the runner reaches, \
                 but the next form here is `{}`. Keep the specifier directly above \
                 the row it guards, with nothing in the gap — this check cannot \
                 tell *which* test form follows, so adjacency is the only part of \
                 \"it guards that row\" that text can hold on to. (`test-end` is \
                 not a row: SRFI 64 discards pending specifiers there, so a \
                 specifier above it guards nothing.)",
                i + 1,
                next.trim()
            );
        }
    }
}

/// The line with any trailing `;` comment removed, ignoring semicolons inside
/// string literals.
///
/// Needed because these files discuss their own skips at length, and a prose
/// mention of `(test-skip 2)` in a trailing comment would otherwise be read as
/// a real one — which fails the adjacency rule while blaming the wrong line.
/// Row names routinely contain no semicolon but may contain anything, so the
/// scan tracks strings rather than cutting at the first `;`.
///
/// Not handled: a literal `#\;` character, which this treats as a comment
/// start. No file contains one, and the machinery to know better is a lexer.
fn code_before_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // A continuation byte of a multi-byte character is never one of
            // these, so walking by byte cannot mistake one for a delimiter.
            b'\\' if in_string => i += 1,
            b'"' => in_string = !in_string,
            b';' if !in_string => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

/// Whether a line begins an assertion a pending specifier can bind to.
///
/// `test-begin`, `test-end` and `test-group` share the `(test-` prefix and are
/// not rows; `test-end` in particular discards the pending specifiers
/// (`lib/srfi/64.scm`), so a specifier directly above it is a dangling one
/// that the count-based guards would otherwise never see.
fn starts_a_test_row(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("(test-")
        && !["(test-end", "(test-begin", "(test-group"]
            .iter()
            .any(|form| line.starts_with(form))
}

/// Every `(test-skip …)` / `(test-expect-fail …)` on one line, as
/// (form name, specifier text, byte offset of the specifier's `(`).
///
/// All of them, not just the first: two specifiers can share a line, and one
/// checked plus one unchecked is worse than neither. The delimiter check after
/// the name stops `(test-skip-everything …)` matching as `test-skip`. The
/// offset is what [`declared_backend_expectations`] uses to find the
/// `cond-expand` clause a specifier sits in, so the two read one grammar.
fn specifiers_in(code: &str) -> Vec<(&'static str, &str, usize)> {
    let mut found = Vec::new();
    for form in ["test-skip", "test-expect-fail"] {
        let opener = format!("({form}");
        let mut from = 0;
        while let Some(at) = code[from..].find(&opener) {
            let at = from + at;
            let after = &code[at + opener.len()..];
            from = at + opener.len();
            match after.chars().next() {
                Some(c) if c.is_whitespace() => {}
                _ => continue, // `(test-skipping`, or `(test-skip)` with no spec
            }
            found.push((form, after.split(')').next().unwrap_or(after).trim(), at));
        }
    }
    found
}

/// What a rejected specifier is, so the failure names the actual mistake.
fn describe(spec: &str) -> String {
    if spec.starts_with('"') {
        "a name, which has to be kept identical to the row beneath it and \
         desyncs silently when the row is renamed"
            .to_string()
    } else if spec.starts_with('(') {
        "a predicate — legal SRFI 64, but this suite pins the count form so the \
         guard below can check it"
            .to_string()
    } else if spec == "0" {
        "zero, which is `(test-match-nth 1 0)` — a predicate that is never true, \
         so it guards nothing while looking like a count"
            .to_string()
    } else {
        "not a positive count".to_string()
    }
}

/// The count form actually skips exactly the next row — on *our* SRFI 64.
///
/// Nothing else pins this. The driver rejects `skip != 0` outright, and every
/// scoped row hides its `test-skip` behind `(cond-expand (patina) (else …))`, so
/// no file in `tests/scheme/` ever evaluates one on Patina. Upstream's own suite
/// does not cover it either: `compat/vendor/srfi-64/test.scm` exercises
/// `test-match-nth` explicitly and the *string* shorthand at "6.2. Shorthand
/// specifiers", and uses the integer shorthand nowhere. So `make-pred`'s
/// `(integer? spec)` branch and `test-match-nth`'s counter could both regress
/// with every scoped row still green here and wrong on chibi and Gauche.
///
/// The skipped row is a deliberate failure, so a skip that lands on the wrong
/// row — or does not happen — shows up as a `fail`, not merely as a count that
/// moved.
#[test]
fn the_count_form_skips_exactly_the_next_row() {
    let counts = run_identically(
        "skip-by-count",
        r#"(import (scheme base) (srfi 64))
           (test-begin "deliberate")
           (test-skip 1)
           (test-assert "skipped, and would fail if it ran" #f)
           (test-assert "the count is spent" #t)
           (test-assert "and stays spent" #t)
           (test-end)"#,
    )
    .expect("the deliberate probe must run on both backends");
    assert_eq!(
        counts,
        Counts {
            pass: 2,
            fail: 0,
            xpass: 0,
            xfail: 0,
            skip: 1
        },
        "`(test-skip 1)` no longer means \"the next row\" on our SRFI 64, which \
         is the form every scoped row in tests/scheme/ uses on chibi and Gauche"
    );
}

/// Every row named in the oracle divergence register still exists.
///
/// `DIVERGENCES.tsv` records, per file and per external implementation, which
/// rows that implementation answers differently and why — the register
/// `scripts/run_suite_oracles.sh` holds the oracles to. That script needs chibi
/// and Gauche installed; this check does not, and it catches the failure the
/// script cannot see coming: a row renamed here leaves a register entry
/// pointing at nothing, and the script would then report the *old* name as "no
/// longer differs" and the *new* one as unregistered — two confusing lines for
/// one rename.
///
/// It also pins the class vocabulary, because the classes are the point. A
/// tally says "3 rows differ" and gives no signal about who should change; the
/// class does, and `oracle-defect` in particular is a record that someone
/// investigated and concluded the *oracle* is wrong, so nobody later matches
/// Patina to it. A typo'd class silently leaves a row unclassified.
#[test]
fn every_registered_divergence_names_a_real_row() {
    const CLASSES: &[&str] = &[
        "latitude",
        "spec-silent",
        "oracle-defect",
        "patina-defect",
        "needs-investigation",
        "incomplete",
    ];
    let path = scheme_dir().join("DIVERGENCES.tsv");
    let register = read(&path).unwrap_or_else(|e| panic!("{e}"));
    let listed: Vec<&str> = SUITE.iter().map(|(n, _)| *n).collect();
    let mut seen = 0;
    // A duplicated entry puts the same row twice into the lane's `expected`
    // list, and `comm` against one actual occurrence then reports it as "no
    // longer differs" — a row that in fact still does, blamed for a
    // copy-paste in a hand-edited file.
    let mut keys: std::collections::HashSet<(String, String, String)> = Default::default();
    // `*` (the file does not complete here) and a named row are mutually
    // exclusive claims about the same pair. The lane resolves the conflict
    // silently in favour of `*` — it takes the incomplete branch and never
    // looks at the rows — so a carefully classified divergence filed against a
    // `*` pair is checked by nothing at all, forever, with a plausible-looking
    // register row to suggest otherwise.
    let mut starred: std::collections::HashSet<(String, String)> = Default::default();
    let mut rowed: std::collections::HashSet<(String, String)> = Default::default();
    // Files are read once each, not once per row: the register is expected to
    // grow, and it has several rows per file already.
    let mut texts: std::collections::HashMap<String, String> = Default::default();

    for (n, line) in register.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            cols.len(),
            5,
            "DIVERGENCES.tsv:{} has {} tab-separated columns, expected exactly 5 \
             (file, oracle, row, class, note): {line:?}. Exactly, not at least: \
             a tab pasted into the note splits it into further columns, and both \
             this check and the script's awk read only the fifth — so the note \
             that carries a class's evidence would silently lose its tail.",
            n + 1,
            cols.len()
        );
        let (file, oracle, row, class) = (cols[0], cols[1], cols[2], cols[3]);
        seen += 1;

        assert!(
            listed.contains(&file),
            "DIVERGENCES.tsv:{} names {file:?}, which is not in SUITE",
            n + 1
        );
        assert!(
            CLASSES.contains(&class),
            "DIVERGENCES.tsv:{} has class {class:?}; the vocabulary is {CLASSES:?}. \
             The class is what says who should change — an unrecognised one \
             leaves the row effectively unclassified.",
            n + 1
        );
        assert!(
            !cols[4].trim().is_empty(),
            "DIVERGENCES.tsv:{} has an empty note. The note carries the evidence; \
             a class without one is an assertion.",
            n + 1
        );
        assert!(
            matches!(oracle, "chibi" | "gauche"),
            "DIVERGENCES.tsv:{} names oracle {oracle:?}, which the lane cannot run",
            n + 1
        );

        assert!(
            keys.insert((file.to_string(), oracle.to_string(), row.to_string())),
            "DIVERGENCES.tsv:{} repeats ({file}, {oracle}, {row:?}). A duplicate \
             makes the lane report that row as \"no longer differs\" while it \
             still does.",
            n + 1
        );

        // `*` means the file does not complete on that oracle, so there is no
        // row to find — but it is still a register key, so uniqueness is
        // checked above this point rather than below it.
        if row == "*" {
            starred.insert((file.to_string(), oracle.to_string()));
            assert_eq!(
                class,
                "incomplete",
                "DIVERGENCES.tsv:{} uses `*` (the file does not complete) but is \
                 classed {class:?}",
                n + 1
            );
            continue;
        }
        assert_ne!(
            class,
            "incomplete",
            "DIVERGENCES.tsv:{} is classed `incomplete` but names a row; \
             `incomplete` is for a whole file, spelled `*`",
            n + 1
        );

        rowed.insert((file.to_string(), oracle.to_string()));

        let text = texts
            .entry(file.to_string())
            .or_insert_with(|| read(&scheme_dir().join(file)).unwrap_or_else(|e| panic!("{e}")));
        assert!(
            text.contains(&format!("\"{row}\"")),
            "DIVERGENCES.tsv:{} names the row {row:?} in {file}, which has no test \
             by that name. Renaming a row leaves its register entry pointing at \
             nothing, and the oracle lane then reports one rename as two \
             mismatches.",
            n + 1
        );
    }

    let conflicts: Vec<_> = starred.intersection(&rowed).collect();
    assert!(
        conflicts.is_empty(),
        "DIVERGENCES.tsv registers both `*` and named rows for {conflicts:?}. \
         Those are mutually exclusive claims about the same pair, and the lane \
         resolves them silently in favour of `*` — so the named rows would be \
         checked by nothing while looking as though they were."
    );

    assert!(
        seen > 0,
        "DIVERGENCES.tsv parsed to zero entries — a register that records nothing \
         passes every check in it"
    );
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
    let counts = run_identically(
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
    )
    .expect("the deliberate probe must run on both backends");
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

/// The backend-scoped expectation is the driver's own mechanism, so it gets a
/// probe of its own, beside the one for the four result kinds: a row that is
/// true on one backend and false on the other, declared to fail on the second,
/// is one `xfail` there and one `pass` here — and
/// [`backends_ran_the_same_rows`] accepts exactly that, and rejects the same
/// counts once the declaration names the wrong backend.
///
/// The row is built on `(features)` rather than on a real divergence, so it
/// stays a probe whatever the backends converge on.
#[test]
fn harness_holds_a_backend_scoped_expectation_to_its_declaration() {
    const PROGRAM: &str = r#"(import (scheme base) (srfi 64))
           (test-begin "deliberate")
           (cond-expand (patina-tree-walker (test-expect-fail 1)) (else))
           (test-assert "true on the vm only" (memq 'patina-vm (features)))
           (test-end)"#;
    let (tw, vm) =
        run_on_both_backends("scoped-probe", PROGRAM).expect("the probe must run on both backends");
    assert_eq!(
        tw,
        Counts {
            pass: 0,
            fail: 0,
            xpass: 0,
            xfail: 1,
            skip: 0
        },
        "the tree-walker should record the declared expected failure"
    );
    assert_eq!(
        vm,
        Counts {
            pass: 1,
            fail: 0,
            xpass: 0,
            xfail: 0,
            skip: 0
        },
        "the vm should pass the row plainly"
    );
    backends_ran_the_same_rows("scoped-probe", PROGRAM, &tw, &vm)
        .expect("a declared expectation matching the recorded one is agreement");

    let misdeclared = PROGRAM.replace("patina-tree-walker", "patina-vm");
    let problem = backends_ran_the_same_rows("scoped-probe", &misdeclared, &tw, &vm)
        .expect_err("the same counts under a declaration naming the other backend");
    assert!(
        problem.contains("declares 0 tree-walker and 1 vm"),
        "the rejection should say what the file declares: {problem}"
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

/// Every `- Ours:` pointer in the Larceny triage doc resolves.
///
/// `scheme_tests/reports/larceny_triage.md` is the open defect queue CLAUDE.md
/// sends people to first, and each family entry names the test that reproduces
/// it. #193 Phase 1 moved those tests out of `larceny_families.rs` into this
/// directory — that file is deleted now — and the pointers rotted a slice at
/// a time — 22 of them named Rust functions that existed nowhere by the
/// time anyone looked. A queue that points at deleted symbols is worse than
/// one that points at nothing: it reads as though someone checked.
///
/// It lives here rather than in a file of its own because a new `tests/*.rs`
/// costs a whole binary (~6 s of link, the thing #193 exists to reduce), and
/// here it sits beside `every_registered_divergence_names_a_real_row`, which
/// does the same job for `DIVERGENCES.tsv`. When the queue is empty the doc is
/// meant to be deleted; this check goes with it, and the empty-doc case below
/// says so rather than passing vacuously.
///
/// **What counts as a pointer**, since the lines are prose and not a table:
/// inside a `- Ours:` line, a backticked token is checked if it looks like a
/// Rust identifier (lowercase, and containing `_` — which is what keeps
/// ordinary prose words like `do` and `map` out) or if it names a file. A
/// double-quoted string is checked as a row name against whichever `.scm`
/// files that line named. Anything else is prose and ignored. The heuristic
/// errs toward silence: a pointer written in some other shape is not checked,
/// which is the failure mode that leaves work undone rather than the one that
/// blocks a PR over a sentence.
#[test]
fn every_triage_pointer_names_something_that_exists() {
    let doc = repo_root().join("scheme_tests/reports/larceny_triage.md");
    let text = match read(&doc) {
        Ok(t) => t,
        // The doc is disposable by its own header. Gone is fine; empty is not,
        // which is the state a half-finished deletion leaves behind.
        Err(_) => return,
    };
    assert!(
        text.contains("- Ours:"),
        "{} has no `- Ours:` lines at all. If the queue is empty the doc is \
         meant to be deleted along with this check, not left as a husk.",
        doc.display()
    );

    let rust_fns: String = files_under(&repo_root().join("crates"))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .filter_map(|p| read(&p).ok())
        .collect();
    let repo = repo_root();
    // Both listings are walked once, not once per pointer: the doc names ~40
    // of them, and the walk they would repeat is the whole test tree.
    let test_files: Vec<PathBuf> = files_under(&repo.join("crates/patina-tests/tests"));
    let mut texts: std::collections::HashMap<String, String> = Default::default();
    let mut checked = 0;

    for (n, line) in text.lines().enumerate() {
        let Some(rest) = line.strip_prefix("- Ours:") else {
            continue;
        };
        let at = format!("{}:{}", doc.display(), n + 1);
        let ticked: Vec<&str> = rest.split('`').skip(1).step_by(2).collect();

        let mut named_files = Vec::new();
        for tok in &ticked {
            let is_path = tok.ends_with(".rs") || tok.ends_with(".scm") || tok.ends_with(".sld");
            if is_path {
                // Written full-path or by basename; both appear in the doc.
                let found = if tok.contains('/') {
                    repo.join(tok).exists()
                } else {
                    test_files
                        .iter()
                        .any(|p| p.file_name().is_some_and(|f| f == *tok))
                };
                assert!(
                    found,
                    "{at} names the file `{tok}`, which does not exist. The test \
                     it held has probably moved into tests/scheme/ — repoint the \
                     entry at the suite file and the row names, as the entries \
                     for families 2, 4 and 19 do."
                );
                if tok.ends_with(".scm") {
                    named_files.push(*tok);
                }
                checked += 1;
            } else if tok.starts_with(|c: char| c.is_ascii_lowercase())
                && tok.contains('_')
                && tok
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                assert!(
                    rust_fns.contains(&format!("fn {tok}")),
                    "{at} names the Rust test `{tok}`, which no longer exists. \
                     Either it was renamed, or #193 moved it into tests/scheme/ \
                     — repoint the entry at the suite file and the row name."
                );
                checked += 1;
            }
        }

        // A row name is only checkable against a file the same line names.
        for row in rest.split('"').skip(1).step_by(2) {
            if named_files.is_empty() {
                continue;
            }
            let needle = format!("{row:?}");
            assert!(
                named_files.iter().any(|f| texts
                    .entry((*f).to_string())
                    .or_insert_with(|| read(&repo.join(f)).unwrap_or_default())
                    .contains(&needle)),
                "{at} names the row {row:?}, which appears in none of the files \
                 that line points at ({named_files:?}). A row renamed in the \
                 suite leaves the queue pointing at a title nothing carries. If \
                 this is a section heading or an aside rather than a row, put it \
                 in backticks — on an `- Ours:` line, double quotes mean a row."
            );
            checked += 1;
        }
    }

    assert!(
        checked > 20,
        "only {checked} triage pointers were checkable, which is fewer than the \
         doc is known to carry — the `- Ours:` shape has probably changed and \
         this check is now reading past most of it."
    );
}
