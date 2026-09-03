//! A generated matrix of control-transfer shapes, scored against reference
//! implementations.
//!
//! Every defect this area has had was found one at a time, by hand-writing a
//! program that turned out to answer wrong — #157, #158, #159, #160, #162,
//! #163, #165, #167 — while the chibi suite read 1226/1226 on both backends
//! through all of them. Twice a fix for one shape broke another that nothing
//! enumerated, and once a fix shipped a regression that a review caught rather
//! than a test.
//!
//! So this file does not test a defect. It enumerates a **space**: the cross
//! product of how a `dynamic-wind` is written, whether it sits in tail
//! position, and how control leaves or re-enters it. A fix that improves one
//! point and breaks another cannot pass. It is `hygiene_matrix.rs`'s idea,
//! one layer down.
//!
//! # The oracle
//!
//! Measured 2026-09-03. The eight `escape` and `reenter` rows are plain R7RS
//! and answer identically on **chibi 0.12, Gauche 0.9.15, Guile 3.0.11 and
//! both Patina backends** — five implementations, so a single `correct`
//! column can stand for the answer. The twelve prompt rows are Guile's:
//! `(ice-9 control)`'s `call-with-prompt` / `abort-to-prompt` are tagged like
//! Patina's, and Guile has R7RS `with-exception-handler` and
//! `raise-continuable` besides, which neither Racket (no `raise-continuable`)
//! nor Gauche (untagged `shift`/`reset`) can offer together.
//!
//! Guile runs the **same program text**, not a translation: the difference is
//! a four-line prelude that spells Patina's prompt API in terms of Guile's,
//! including the handler argument order. A matrix whose oracle ran a
//! *different* program would be measuring the transcription.
//!
//! # Re-measuring
//!
//! [`dump_programs`] writes every program to a directory, so the text an
//! oracle is fed is the text this test runs. Nothing else guarantees that.
//!
//! # Reading a row
//!
//! Each row records `correct` and what each backend answers **today**. A row
//! where an actual differs from `correct` is a live defect, pinned so it
//! cannot drift silently, and named in `issue`.
//!
//! - **fix a defect** → its row goes red with the new (correct) answer.
//!   Update `vm`/`tw` and the count in [`the_defect_count_is_what_it_says`].
//! - **break a working shape** → its row goes red too. That is the direction
//!   that has cost this area the most.
//!
//! The observable is always `(list r (reverse log))` — the value the transfer
//! produced *and* the thunks it ran, in order. Both halves are needed: the
//! defects here show up as a lost value with a correct log (#157, #160) at
//! least as often as a wrong log.
//!
//! # What the tree-walker does here
//!
//! It has no prompt API at all, so the twelve prompt rows record the
//! `Undefined variable` it raises. That is not a wrong answer, it is an
//! absent feature, and [`the_defect_count_is_what_it_says`] separates the two:
//! it asserts the tree-walker matches on **every shape it can run**, and that
//! the only rows it misses are exactly the ones needing prompts. When it
//! grows a prompt API those twelve rows go red together, which is the signal
//! to measure them rather than a failure.
//!
//! # Adding axes
//!
//! The axes here are the ones the defect history names. Worth adding next,
//! roughly in order of value:
//!
//! - **where inside the extent a continuation is captured** — the body, the
//!   `before` thunk, the `after` thunk. Only [`Transfer::ResumeThenReenterThunk`]
//!   reaches into a thunk today, and that one axis produced #157, #159, #165
//!   and #167. As a separate `Capture` axis it would multiply these rows by
//!   three; as one more `Transfer` it stays cheap.
//! - **an exception crossing the extent** — `raise` and `guard` interacting
//!   with the thunks, which is Track L §6's `finally` rule and has its own
//!   tests but no enumeration.
//! - **nested extents**, where the common-prefix rule is what is being
//!   tested rather than a single extent's bookkeeping.
//! - **invoking a captured continuation more than once**, which is where a
//!   composable continuation differs from a full one.
//!
//! An axis must move one thing. [`Position`] is a `((lambda () …))` either
//! way, differing only in whether the extent is the tail expression — a
//! version that made the non-tail case a different *form* would have measured
//! the form axis twice.

mod common;

use common::{try_eval_program_tree_walker, try_eval_program_vm};

/// What the tree-walker answers for every shape that needs a prompt: it has
/// no prompt API, so the program does not run at all. Recorded rather than
/// skipped — when the tree-walker grows one, these rows go red together and
/// ask to be measured.
const NO_PROMPTS: &str = "<error>: Backend error: Undefined variable: \
patina.internal.control/call-with-continuation-prompt";

/// How a rejection is rendered, so a row that starts failing says which.
const ERROR: &str = "<error>";

/// How the `dynamic-wind` call is reached.
#[derive(Clone, Copy, PartialEq)]
enum Extent {
    /// Written in head position, so pass 5 compiles it to
    /// `PushWind`/`PopWind` with the thunks as ordinary `Call`s.
    Head,
    /// Reached as a *value*, so the runtime runs the same instructions in a
    /// stub frame (`value_wind_stub`). Issue #157 lived on this axis alone.
    Value,
}

/// Whether the extent is in tail position of the body it sits in.
#[derive(Clone, Copy, PartialEq)]
enum Position {
    Tail,
    NonTail,
}

/// How control leaves, or re-enters, the extent's body.
#[derive(Clone, Copy, PartialEq)]
enum Transfer {
    /// Jump *out* to a continuation captured outside the extent.
    Escape,
    /// Jump back *in* to a continuation captured inside the body.
    Reenter,
    /// Abort to a prompt established outside the extent.
    Abort,
    /// Invoke the composable continuation that abort handed the handler.
    Resume,
    /// Invoke it, having also captured a continuation in the extent's
    /// `before` thunk, and re-enter that.
    ResumeThenReenterThunk,
}

impl Extent {
    fn name(self) -> &'static str {
        match self {
            Extent::Head => "head",
            Extent::Value => "value",
        }
    }
    /// The operator: the syntactic name, or a variable bound to it.
    fn operator(self) -> &'static str {
        match self {
            Extent::Head => "dynamic-wind",
            Extent::Value => "dw",
        }
    }
}

impl Position {
    fn name(self) -> &'static str {
        match self {
            Position::Tail => "tail",
            Position::NonTail => "non-tail",
        }
    }
}

impl Transfer {
    fn name(self) -> &'static str {
        match self {
            Transfer::Escape => "escape",
            Transfer::Reenter => "reenter",
            Transfer::Abort => "abort",
            Transfer::Resume => "resume",
            Transfer::ResumeThenReenterThunk => "resume+thunk-reenter",
        }
    }
    /// Whether the shape needs the prompt API, which only the VM has.
    fn needs_prompts(self) -> bool {
        matches!(
            self,
            Transfer::Abort | Transfer::Resume | Transfer::ResumeThenReenterThunk
        )
    }
}

/// One point in the space.
struct Shape {
    extent: Extent,
    position: Position,
    transfer: Transfer,
    /// What the reference implementations answer.
    correct: &'static str,
    /// What the VM answers today.
    vm: &'static str,
    /// What the tree-walker answers today.
    tw: &'static str,
    /// The issue a wrong row is a face of, or `""` when the row is correct.
    issue: &'static str,
}

impl Shape {
    fn name(&self) -> String {
        format!(
            "{}/{}/{}",
            self.extent.name(),
            self.position.name(),
            self.transfer.name()
        )
    }
}

/// The extent, with `body` as its body thunk and `after` as its after-thunk.
fn extent(shape: &Shape, before: &str, body: &str, after: &str) -> String {
    format!(
        "({} (lambda () {before}) (lambda () {body}) (lambda () {after}))",
        shape.extent.operator()
    )
}

/// Put `expr` in tail position of a lambda body, or not.
///
/// The axis moves one thing: both spellings call a thunk, and only the
/// extent's position inside it differs. Issue #158's body-abort row failed
/// only in the non-tail spelling — the tail one pops the frame first and
/// happened to survive — so a matrix with just one of them would have scored
/// that defect green.
fn positioned(shape: &Shape, expr: &str) -> String {
    match shape.position {
        Position::Tail => format!("((lambda () {expr}))"),
        Position::NonTail => format!("((lambda () (list 'w {expr})))"),
    }
}

/// The program for one shape.
///
/// Every program ends in `(list r (reverse log))`: the value the transfer
/// produced, and the thunks it ran, in order. Both halves are needed — the
/// defects in this area show up as a lost value with a correct log
/// (#157, #160) at least as often as a wrong log.
fn program(shape: &Shape) -> String {
    let prelude = "(define dw dynamic-wind)\n\
                   (define log '())\n\
                   (define (note x) (set! log (cons x log)))";
    let inn = "(note 'in)";
    let out = "(note 'out)";
    let body = match shape.transfer {
        Transfer::Escape => "(k 'escaped)",
        Transfer::Reenter => "(call/cc (lambda (c) (set! k c) 'first))",
        Transfer::Abort => "(abort-current-continuation t 'ab)",
        Transfer::Resume | Transfer::ResumeThenReenterThunk => {
            "(list 'got (abort-current-continuation t 'ab))"
        }
    };
    // Only the last transfer captures in a thunk; everything else keeps the
    // thunks to one `note` each, so the axis moves one thing.
    let before = match shape.transfer {
        Transfer::ResumeThenReenterThunk => {
            "(note 'in-1) (call/cc (lambda (c) (set! kt c))) (note 'in-2)"
        }
        _ => inn,
    };
    let core = positioned(shape, &extent(shape, before, body, out));
    match shape.transfer {
        Transfer::Escape => {
            format!("{prelude}\n(define r (call/cc (lambda (k) {core})))\n(list r (reverse log))")
        }
        Transfer::Reenter => format!(
            "{prelude}\n(define k #f)\n(define n 0)\n(define r {core})\n\
             (if (< n 1) (begin (set! n 1) (k 'again)))\n(list r (reverse log))"
        ),
        Transfer::Abort => format!(
            "{prelude}\n(define t (make-continuation-prompt-tag 'p))\n\
             (define r (call-with-continuation-prompt (lambda () {core}) t (lambda (v k) (list 'h v))))\n\
             (list r (reverse log))"
        ),
        Transfer::Resume => format!(
            "{prelude}\n(define t (make-continuation-prompt-tag 'p))\n(define k* #f)\n\
             (define cap (call-with-continuation-prompt (lambda () {core}) t (lambda (v k) (set! k* k) 'cap)))\n\
             (define r (k* 'resumed))\n(list r (reverse log))"
        ),
        Transfer::ResumeThenReenterThunk => format!(
            "{prelude}\n(define t (make-continuation-prompt-tag 'p))\n(define k* #f)\n\
             (define kt #f)\n(define n 0)\n\
             (define cap (call-with-continuation-prompt (lambda () {core}) t (lambda (v k) (set! k* k) 'cap)))\n\
             (define r (k* 'resumed))\n\
             (if (< n 1) (begin (set! n 1) (kt 'again)))\n(list r (reverse log))"
        ),
    }
}

/// Every shape, with both backends' current answers.
///
/// First measured 2026-09-03 on `b240d217`: four rows wrong, all of them
/// #167, and all on the one transfer that re-enters a continuation captured
/// inside a wind thunk.
#[rustfmt::skip]
const MATRIX: &[Shape] = &[
    // ---- escape: a jump OUT of the body ---------------------------------
    // Unanimous across five implementations, and the baseline the other
    // transfers are read against: one entry, one exit, value delivered.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Escape,
            correct: "(escaped (in out))", vm: "(escaped (in out))", tw: "(escaped (in out))", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Escape,
            correct: "(escaped (in out))", vm: "(escaped (in out))", tw: "(escaped (in out))", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Escape,
            correct: "(escaped (in out))", vm: "(escaped (in out))", tw: "(escaped (in out))", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Escape,
            correct: "(escaped (in out))", vm: "(escaped (in out))", tw: "(escaped (in out))", issue: "" },

    // ---- reenter: a jump back IN ----------------------------------------
    // The extent is left and entered again, so both thunks run twice. The
    // non-tail rows carry the `(w …)` wrapper the tail rows do not, which is
    // the whole of that axis: same program, one frame more.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Reenter,
            correct: "(again (in out in out))", vm: "(again (in out in out))", tw: "(again (in out in out))", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Reenter,
            correct: "((w again) (in out in out))", vm: "((w again) (in out in out))", tw: "((w again) (in out in out))", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Reenter,
            correct: "(again (in out in out))", vm: "(again (in out in out))", tw: "(again (in out in out))", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Reenter,
            correct: "((w again) (in out in out))", vm: "((w again) (in out in out))", tw: "((w again) (in out in out))", issue: "" },

    // ---- abort: out to a prompt ----------------------------------------
    // #158's row. On `7e696892` the non-tail spelling *panicked* (`no active
    // frame`) while the tail one passed, which is why the position axis is
    // here at all.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: NO_PROMPTS, issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: NO_PROMPTS, issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: NO_PROMPTS, issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: NO_PROMPTS, issue: "" },

    // ---- resume: invoke the composable continuation ---------------------
    // #160's row: the value has to reach the hole *and* come back out. The
    // captured extent is re-entered, so the log doubles.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Resume,
            correct: "((got resumed) (in out in out))", vm: "((got resumed) (in out in out))", tw: NO_PROMPTS, issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Resume,
            correct: "((w (got resumed)) (in out in out))", vm: "((w (got resumed)) (in out in out))", tw: NO_PROMPTS, issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Resume,
            correct: "((got resumed) (in out in out))", vm: "((got resumed) (in out in out))", tw: NO_PROMPTS, issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Resume,
            correct: "((w (got resumed)) (in out in out))", vm: "((w (got resumed)) (in out in out))", tw: NO_PROMPTS, issue: "" },

    // ---- resume, then re-enter a continuation captured in a thunk -------
    // Issue #167, on all four spellings. The resumed value is lost (`()` for
    // `(got resumed)`) and the extent is left unbalanced — the last `out`
    // never runs. `invoke_delimited` runs these thunks on a nested Rust loop,
    // so the re-entry has no pc to come back to; the issue records why
    // routing them through `step_wind_jump`, which is what fixed the abort's
    // thunks in #165, is *not* the fix here.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(() (in-1 in-2 out in-1 in-2 out in-2))", tw: NO_PROMPTS, issue: "#167" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(() (in-1 in-2 out in-1 in-2 out in-2))", tw: NO_PROMPTS, issue: "#167" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(() (in-1 in-2 out in-1 in-2 out in-2))", tw: NO_PROMPTS, issue: "#167" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(() (in-1 in-2 out in-1 in-2 out in-2))", tw: NO_PROMPTS, issue: "#167" },
];

/// Write every program to `$CONTROL_FLOW_MATRIX_DUMP/<name>.scm`.
///
/// Ignored by default: it produces no assertion, it exists so the table can be
/// **re-measured** rather than re-derived. The programs an oracle is fed must
/// be the programs the test runs, and the only way to guarantee that is to
/// take them from this generator:
///
/// ```text
/// CONTROL_FLOW_MATRIX_DUMP=/tmp/cfm cargo test -p patina-tests \
///     --test control_flow_matrix -- --ignored dump_programs
/// ```
#[test]
#[ignore]
fn dump_programs() {
    let dir = std::env::var("CONTROL_FLOW_MATRIX_DUMP")
        .expect("set CONTROL_FLOW_MATRIX_DUMP to a directory");
    std::fs::create_dir_all(&dir).expect("create dump directory");
    for shape in MATRIX {
        let path = format!("{dir}/{}.scm", shape.name().replace('/', "_"));
        std::fs::write(&path, program(shape)).expect("write program");
    }
}

/// Run one shape on one backend, rendering a rejection as [`ERROR`].
///
/// The message is kept rather than collapsed to a token: the twelve
/// tree-walker rows are errors *by design*, and a bare marker would not
/// distinguish "no prompt API" from a crash in one.
fn answer(code: &str, vm: bool) -> String {
    let ok = if vm {
        try_eval_program_vm(code)
    } else {
        try_eval_program_tree_walker(code)
    };
    ok.unwrap_or_else(|e| format!("{ERROR}: {}", e.lines().next().unwrap_or("").trim()))
}

/// Every shape answers what the table says it answers, on both backends.
///
/// The point is that this fails in *both* directions: fixing a defect turns
/// its row red just as breaking a working shape does. A red row is never
/// "just update the table" — read which direction it moved first.
#[test]
fn shapes_score_as_recorded() {
    let mut moved = Vec::new();
    for shape in MATRIX {
        let code = program(shape);
        for (backend, recorded) in [("vm", shape.vm), ("tree-walker", shape.tw)] {
            let got = answer(&code, backend == "vm");
            if got != recorded {
                let direction = if got == shape.correct {
                    "FIXED — it now matches the reference implementations"
                } else if recorded == shape.correct {
                    "REGRESSED — it used to match the reference implementations"
                } else {
                    "changed, and is still wrong"
                };
                moved.push(format!(
                    "  {} [{backend}] {direction}\n    recorded {recorded}\n    got      {got}\n\
                     \x20   correct  {}\n    program:\n{}",
                    shape.name(),
                    shape.correct,
                    code.lines()
                        .map(|l| format!("      {l}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }
    }
    assert!(
        moved.is_empty(),
        "{} control-flow shape(s) no longer answer what the matrix records.\n\n{}\n\n\
         Update the row(s) in MATRIX and the count in \
         `the_defect_count_is_what_it_says`.",
        moved.len(),
        moved.join("\n\n")
    );
}

/// How many shapes each backend gets wrong, asserted as a number.
///
/// A count is what the chibi suite cannot give: it has read 1226/1226 on both
/// backends through every defect this matrix exists for. This number can only
/// move deliberately.
#[test]
fn the_defect_count_is_what_it_says() {
    let vm_wrong: Vec<String> = MATRIX
        .iter()
        .filter(|s| s.vm != s.correct)
        .map(|s| s.name())
        .collect();
    // Four: issue #167, one per spelling of the same program. That it is four
    // and not one is the matrix earning its keep — the issue was filed from a
    // single repro.
    assert_eq!(
        vm_wrong.len(),
        4,
        "VM shapes wrong: {vm_wrong:?} (expected the four #167 rows)"
    );
    assert!(
        vm_wrong.iter().all(|n| n.ends_with("resume+thunk-reenter")),
        "the VM's wrong rows should all be #167's: {vm_wrong:?}"
    );

    // The tree-walker matches on every shape it can run, and the only rows it
    // misses are exactly the ones needing a prompt API it does not have.
    for shape in MATRIX {
        if shape.transfer.needs_prompts() {
            assert_eq!(
                shape.tw,
                NO_PROMPTS,
                "{}: the tree-walker has no prompts, so this row records that",
                shape.name()
            );
        } else {
            assert_eq!(
                shape.tw,
                shape.correct,
                "{}: the tree-walker runs this shape and must match",
                shape.name()
            );
        }
    }

    // Every wrong row names its issue, so the table doubles as the index from
    // defect to surface count.
    for shape in MATRIX {
        let vm_correct = shape.vm == shape.correct;
        assert_eq!(
            !vm_correct,
            !shape.issue.is_empty(),
            "{}: `issue` must be set exactly on rows the VM gets wrong",
            shape.name()
        );
    }
}

/// The space is a full cross product, with no row written twice.
///
/// Cheap, and it is the check a hand-maintained table of 20 rows most needs:
/// a duplicated row scores twice and a missing one scores not at all, and
/// neither shows up as a failure anywhere else.
#[test]
fn the_matrix_is_a_complete_cross_product() {
    let mut seen: Vec<String> = MATRIX.iter().map(|s| s.name()).collect();
    let total = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), total, "duplicate rows in MATRIX");
    assert_eq!(
        total,
        2 * 2 * 5,
        "MATRIX should hold every (extent, position, transfer) combination"
    );
}
