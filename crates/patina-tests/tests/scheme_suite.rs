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
//! **That property is exercised by hand, and nothing here re-checks it.** Each
//! file's header records the tallies its oracles produced, on a stated date,
//! and those numbers are what the next person runs it against — but this driver
//! runs only the two Patina backends, and `run_chibi_tests.sh` covers the chibi
//! R7RS suite rather than `tests/scheme/`. So a row edited after the header was
//! written leaves the claim stale with nothing to catch it. All 14 files make
//! such a claim — every one names chibi or Gauche — and four carry an explicit
//! pass/fail tally: `reader/at-identifiers.scm`,
//! `reader/vertical-bar-identifiers.scm`, `data/conversion.scm` and
//! `data/circular-data.scm`. Closing this is what Phase 3 is for; until then the
//! rule is that a PR touching a file's rows re-measures its header.
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
    ("control/cps-features.scm", 50),
    ("control/internal-escape-boundaries.scm", 11),
    ("control/parameters.scm", 18),
    ("control/tail-recursion.scm", 36),
    ("control/values.scm", 5),
    ("control/wind-thunk-exceptions.scm", 14),
    ("data/circular-data.scm", 36),
    ("data/case-mapping.scm", 2),
    ("data/conversion.scm", 62),
    ("data/numeric-operations.scm", 38),
    ("expansion/define-values.scm", 14),
    ("expansion/ellipsis.scm", 3),
    ("expansion/let-values.scm", 1),
    ("expansion/quasiquote.scm", 1),
    ("reader/at-identifiers.scm", 11),
    ("reader/line-endings.scm", 4),
    ("reader/unicode-identifiers.scm", 20),
    ("reader/vertical-bar-identifiers.scm", 32),
    ("stdlib/eval.scm", 1),
    ("stdlib/lazy-evaluation.scm", 32),
    ("stdlib/list.scm", 6),
    ("stdlib/process-context.scm", 12),
    ("stdlib/scheme-r5rs.scm", 20),
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

/// Run `program` on both backends, requiring them to agree.
///
/// Agreement is checked on the whole count vector, not just on "did it pass".
/// Two backends can reach zero failures having run different numbers of
/// assertions — a `cond-expand` that skips a group on one of them would do
/// exactly that — and that divergence is the kind this suite exists to expose.
/// Both backends' counts, or the first problem that stopped one of them.
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
fn run_on_both_backends(label: &str, program: &str) -> Result<Counts, String> {
    let tw = run_on(
        &common::tree_walker_interpreter(),
        &format!("{label} (tree-walker)"),
        program,
    )?;
    let vm = run_on(&common::vm_interpreter(), &format!("{label} (vm)"), program)?;
    if tw != vm {
        return Err(format!(
            "[{label}] the backends disagree on what ran: tree-walker {tw:?}, vm {vm:?}"
        ));
    }
    Ok(vm)
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
        let path = scheme_dir().join(name);
        let counts = match read(&path).and_then(|src| run_on_both_backends(name, &src)) {
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
            for (form, spec) in &specs {
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
                next.trim_start().starts_with("(test-"),
                "[{name}:{}] the count binds to the next test the runner reaches, \
                 but the next form here is `{}`. Keep the specifier directly above \
                 the row it guards, with nothing in the gap — this check cannot \
                 tell *which* test form follows, so adjacency is the only part of \
                 \"it guards that row\" that text can hold on to.",
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

/// Every `(test-skip …)` / `(test-expect-fail …)` on one line, as
/// (form name, specifier text).
///
/// All of them, not just the first: two specifiers can share a line, and one
/// checked plus one unchecked is worse than neither. The delimiter check after
/// the name stops `(test-skip-everything …)` matching as `test-skip`.
fn specifiers_in(code: &str) -> Vec<(&'static str, &str)> {
    let mut found = Vec::new();
    for form in ["test-skip", "test-expect-fail"] {
        let opener = format!("({form}");
        let mut rest = code;
        while let Some(at) = rest.find(&opener) {
            let after = &rest[at + opener.len()..];
            rest = after;
            match after.chars().next() {
                Some(c) if c.is_whitespace() => {}
                _ => continue, // `(test-skipping`, or `(test-skip)` with no spec
            }
            found.push((form, after.split(')').next().unwrap_or(after).trim()));
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
    let counts = run_on_both_backends(
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
