//! A generated matrix of control-transfer shapes, scored against reference
//! implementations.
//!
//! Every defect this area has had was found one at a time, by hand-writing a
//! program that turned out to answer wrong — issues #157, #159, #160, #162,
//! #163, #165 and #167, closed by PRs #158, #161, #164, #166 and #172 — while the
//! chibi suite read 1226/1226 on both backends throughout. Twice a fix for one
//! shape broke another that nothing enumerated, and once a fix shipped a
//! regression a review caught rather than a test.
//!
//! So this file does not test a defect. It enumerates a **space**: the cross
//! product of how a `dynamic-wind` is written, whether it sits in tail
//! position, and how control leaves or re-enters it. A fix that improves one
//! point and breaks another cannot pass. It is `hygiene_matrix.rs`'s idea, one
//! layer down.
//!
//! # The oracle
//!
//! Measured 2026-09-03 against **chibi 0.12, Gauche 0.9.15, Guile 3.0.11 and
//! Racket 9.3** — external implementations only. Patina's own backends are the
//! system under test and are not counted among them.
//!
//! How many of the four back a given row is recorded **per row**, in
//! [`Shape::oracles`], because it varies from four down to one and a single
//! headline number would let the thinnest rows borrow the confidence of the
//! thickest:
//!
//! | Transfer family | Supporting oracles |
//! |---|---|
//! | none, escape | chibi, gauche, guile, racket |
//! | reenter, reenter-before, reenter-after, jump+before-reenter | chibi, gauche, guile |
//! | escape+after-reenter | gauche, guile (Chibi differs; see below) |
//! | abort/resume from body, before, or after | guile, racket |
//! | abort+after-reenter, resume+thunk-reenter, resume+after-reenter | guile |
//!
//! The ordinary before/after rows and the remaining #171 capture-site rows
//! were measured 2026-09-11 with `dump_programs`, using the versions above.
//! Racket rejects full re-entry through a module-level `define` (assignment
//! to a constant); that is an unusable oracle for these programs, not an
//! answer about wind traversal. Chibi and Gauche have no tagged prompt API,
//! so no prompt programs are emitted for them.
//!
//! Chibi **does answer differently** on all four `escape+after-reenter` rows:
//! `(escaped (in out-1 out-2 in out-2))`. Gauche and Guile answer
//! `(escaped (in out-1 out-2 out-2))`, which is the recorded expectation:
//! the exiting record was already popped when its after-thunk captured, so
//! resuming that thunk's remainder does not re-enter the extent. Chibi is
//! deliberately excluded from those rows' `oracles`, not counted as agreement.
//!
//! Every oracle runs the **same program text**. What differs is a prelude:
//! an import line, a `write` around the final expression, and for Guile and
//! Racket a shim spelling Patina's prompt API in terms of theirs. A matrix
//! whose oracle ran a *different* program would be measuring the
//! transcription, so where a construct is not portable the generator changes —
//! `when` rather than a one-armed `if`, because Racket rejects the latter.
//!
//! # Re-measuring
//!
//! [`dump_programs`] writes every program out per oracle, ready to run. That
//! is the only thing guaranteeing the text an oracle is fed is the text this
//! test runs.
//!
//! # Reading a row
//!
//! Each row records `correct` and what each backend answers **today**. A row
//! where an actual differs from `correct` is a live defect, pinned so it
//! cannot drift silently, and named in `issue`.
//!
//! - **fix a defect** → its row goes red with the new (correct) answer. Update
//!   `vm`/`tw` and the count in [`the_defect_count_is_what_it_says`].
//! - **break a working shape** → its row goes red too. That is the direction
//!   that has cost this area the most.
//!
//! The observable is always `(list r (reverse log))` — the value the transfer
//! produced *and* the thunks it ran, in order. Both halves are needed: the
//! defects here show up as a lost value with a correct log (#157, #160) at
//! least as often as a wrong log.
//!
//! # What the matrix has found
//!
//! On its first run, the four `resume+thunk-reenter` rows. That was issue
//! #167, already filed from a single repro — so the enumeration found no *new*
//! defect. What it added is that no spelling of the program avoided it: both
//! extent forms and both positions failed identically, which the one repro
//! could not say and which is what the fix had to hold against. All four went
//! green together when it landed, and the test reported them as `FIXED` rather
//! than as a failure to be explained — which is the half of "fails in both
//! directions" that only gets exercised on a good day.
//!
//! # The tree-walker
//!
//! It had no prompt API until 2026-09-04, and the twelve prompt rows recorded
//! an `UNSUPPORTED` classification instead of an answer — an absent feature,
//! kept apart from a wrong one. They went red together when the API landed
//! (issue #169), each reporting `FIXED` with the reference answer, which is
//! the "request to measure them" this header used to promise; now every row
//! records an answer on both backends. The two backends reach those answers
//! by different mechanisms — the VM's prompt is a depth in three stacks, the
//! tree-walker's a boundary value in the continuation (`cps_eval/prompts.rs`)
//! — which is what makes their agreement here worth something.
//!
//! No row records a wrong answer today, on either backend, and none records
//! an absent feature. `common/mod.rs` names this file as one of the two
//! matrices the backend-scoped-expectation grep does not see; the inventory
//! here is empty, and the file is what keeps it that way.
//!
//! # Capture-site coverage (#171)
//!
//! The 64 rows are 2 extent forms × 2 positions × 16 transfers. Capture site
//! is expressed by transfer variants, avoiding meaningless combinations such
//! as a capture site for `None` or a before-thunk run by abort's exit travel.
//!
//! | Context | Body | Before | After |
//! |---|---|---|---|
//! | Ordinary full capture and later re-entry | Reenter | ReenterBefore | ReenterAfter |
//! | Abort's composable capture | Abort | AbortFromBefore | AbortFromAfter |
//! | Resume that composable capture | Resume | ResumeFromBefore | ResumeFromAfter |
//! | Full capture during a full jump's travel | — | JumpThenReenterBefore | EscapeThenReenterAfter |
//! | Full capture during abort travel | — | — | AbortThenReenterAfter |
//! | Full capture while running a resumed region | — | ResumeThenReenterThunk | ResumeThenReenterAfter |
//!
//! `JumpThenReenterBefore` and `ResumeThenReenterAfter` use a phase counter
//! to capture only during the second entry. `ResumeThenReenterThunk` refreshes
//! its saved continuation on re-entry. Thunks log before and after capture,
//! so restarting
//! the thunk cannot masquerade as resuming its remainder. The jump/abort value
//! is separate from the value delivered to the thunk continuation.
//!
//! No new Patina divergence was found in the 32 rows that completed #171.
//!
//! # Adding axes
//!
//! Further axes worth adding:
//!
//! - **an exception crossing the extent** — `raise` and `guard` interacting
//!   with the thunks, which is Track L §6's `finally` rule and has its own
//!   tests but no enumeration.
//! - **nested extents**, where the common-prefix rule is what is being tested
//!   rather than a single extent's bookkeeping.
//! - **invoking a captured continuation more than once**, which is where a
//!   composable continuation differs from a full one.
//!
//! An axis must move one thing. [`Position`] is a `((lambda () …))` either
//! way, differing only in whether the extent is the tail expression — a
//! version that made the non-tail case a different *form* would have measured
//! the form axis twice.

mod common;

use common::{try_eval_program_tree_walker, try_eval_program_vm};

/// How a rejection is rendered, so a row that starts failing says which.
/// Composed into messages rather than repeated as a literal.
///
/// The message is kept rather than collapsed to a token, and never compared
/// against a recorded value: `common/mod.rs` says why text is never compared
/// — "pinning prose would turn every message improvement into a test break".
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
    /// No transfer: the body returns normally.
    ///
    /// The baseline the other rows are read against. Without it every cell
    /// entangles the extent's own bookkeeping with a transfer crossing it, and
    /// a regression in ordinary result delivery — `PopWind` running before the
    /// after-thunk, `value_wind_stub`'s `Return` clobbering its destination —
    /// would move every other row at once with nothing to say which half
    /// broke.
    None,
    /// Jump *out* to a continuation captured outside the extent.
    Escape,
    /// Jump back *in* to a continuation captured inside the body.
    Reenter,
    /// Re-enter a full continuation captured during ordinary entry.
    ReenterBefore,
    /// Re-enter a full continuation captured during ordinary exit.
    ReenterAfter,
    /// Abort to a prompt established outside the extent.
    Abort,
    /// Invoke the composable continuation that abort handed the handler.
    Resume,
    /// Invoke it, having also captured a continuation in the extent's
    /// `before` thunk, and re-enter that.
    ResumeThenReenterThunk,
    /// Capture in a before thunk only when a full jump re-enters the extent.
    JumpThenReenterBefore,
    /// Capture in the after thunk run by an escaping full continuation.
    EscapeThenReenterAfter,
    /// Capture in the after thunk run by abort travel, then replay its remainder.
    AbortThenReenterAfter,
    /// Capture in the after thunk of a resumed composable region.
    ResumeThenReenterAfter,
    /// Abort captures the composable continuation inside the before thunk.
    AbortFromBefore,
    /// Abort captures the composable continuation inside the after thunk.
    AbortFromAfter,
    /// Resume the composable continuation captured inside the before thunk.
    ResumeFromBefore,
    /// Resume the composable continuation captured inside the after thunk.
    ResumeFromAfter,
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
            Transfer::None => "none",
            Transfer::Escape => "escape",
            Transfer::Reenter => "reenter",
            Transfer::ReenterBefore => "reenter-before",
            Transfer::ReenterAfter => "reenter-after",
            Transfer::Abort => "abort",
            Transfer::Resume => "resume",
            Transfer::ResumeThenReenterThunk => "resume+thunk-reenter",
            Transfer::JumpThenReenterBefore => "jump+before-reenter",
            Transfer::EscapeThenReenterAfter => "escape+after-reenter",
            Transfer::AbortThenReenterAfter => "abort+after-reenter",
            Transfer::ResumeThenReenterAfter => "resume+after-reenter",
            Transfer::AbortFromBefore => "abort-from-before",
            Transfer::AbortFromAfter => "abort-from-after",
            Transfer::ResumeFromBefore => "resume-from-before",
            Transfer::ResumeFromAfter => "resume-from-after",
        }
    }
    /// Whether the shape needs the prompt API — which chibi and Gauche do not
    /// have, so [`dump_programs`] writes no file for them, and which the
    /// tree-walker did not have until 2026-09-04.
    fn needs_prompts(self) -> bool {
        matches!(
            self,
            Transfer::Abort
                | Transfer::Resume
                | Transfer::ResumeThenReenterThunk
                | Transfer::AbortThenReenterAfter
                | Transfer::ResumeThenReenterAfter
                | Transfer::AbortFromBefore
                | Transfer::AbortFromAfter
                | Transfer::ResumeFromBefore
                | Transfer::ResumeFromAfter
        )
    }
}

/// Every variant of each axis, so the completeness check is derived from the
/// enums rather than from a number a reader has to keep in step. Adding a
/// `Transfer` and forgetting to list it here is caught by the exhaustive
/// `match` in [`Transfer::name`], and forgetting to add its rows is caught by
/// [`the_matrix_is_a_complete_cross_product`].
const EXTENTS: &[Extent] = &[Extent::Head, Extent::Value];
const POSITIONS: &[Position] = &[Position::Tail, Position::NonTail];
const TRANSFERS: &[Transfer] = &[
    Transfer::None,
    Transfer::Escape,
    Transfer::Reenter,
    Transfer::ReenterBefore,
    Transfer::ReenterAfter,
    Transfer::Abort,
    Transfer::Resume,
    Transfer::ResumeThenReenterThunk,
    Transfer::JumpThenReenterBefore,
    Transfer::EscapeThenReenterAfter,
    Transfer::AbortThenReenterAfter,
    Transfer::ResumeThenReenterAfter,
    Transfer::AbortFromBefore,
    Transfer::AbortFromAfter,
    Transfer::ResumeFromBefore,
    Transfer::ResumeFromAfter,
];

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
    /// The external implementations that answer `correct` for **this** row.
    ///
    /// Per row, not per file: the header records both unsupported programs
    /// and the Chibi disagreement. A single number across the table
    /// would let a row backed by one implementation borrow the confidence of
    /// a row backed by four.
    oracles: &'static str,
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
/// `when` rather than a one-armed `if`: both are R7RS, and Racket rejects the
/// latter. The point of the dump is that every oracle runs *this* text, so a
/// construct one of them cannot parse is a transcription difference smuggled
/// into the program.
fn program(shape: &Shape) -> String {
    let prelude = "(define dw dynamic-wind)\n\
                   (define log '())\n\
                   (define (note x) (set! log (cons x log)))";
    let inn = "(note 'in)";
    let out = "(note 'out)";
    let body = match shape.transfer {
        Transfer::None | Transfer::ReenterBefore | Transfer::ReenterAfter => "'body",
        Transfer::Escape | Transfer::EscapeThenReenterAfter => "(k 'escaped)",
        Transfer::Reenter | Transfer::JumpThenReenterBefore => {
            "(call/cc (lambda (c) (set! k c) 'first))"
        }
        Transfer::Abort | Transfer::AbortThenReenterAfter => "(abort-current-continuation t 'ab)",
        Transfer::AbortFromBefore
        | Transfer::AbortFromAfter
        | Transfer::ResumeFromBefore
        | Transfer::ResumeFromAfter => "'body",
        Transfer::Resume | Transfer::ResumeThenReenterThunk | Transfer::ResumeThenReenterAfter => {
            "(list 'got (abort-current-continuation t 'ab))"
        }
    };
    // Thunk-capture transfers log both sides of the capture, distinguishing
    // resuming its remainder from incorrectly restarting the whole thunk.
    let capture = "(call/cc (lambda (c) (set! k c)))";
    let before = match shape.transfer {
        Transfer::JumpThenReenterBefore => {
            "(note 'in-1) (when (= n 1) (call/cc (lambda (c) (set! kt c)))) (note 'in-2)"
        }
        Transfer::AbortFromBefore | Transfer::ResumeFromBefore => {
            "(note 'in-1) (abort-current-continuation t 'ab) (note 'in-2)"
        }
        Transfer::ReenterBefore => "(note 'in-1) (call/cc (lambda (c) (set! k c))) (note 'in-2)",
        Transfer::ResumeThenReenterThunk => {
            "(note 'in-1) (call/cc (lambda (c) (set! kt c))) (note 'in-2)"
        }
        _ => inn,
    };
    let after_capture = format!("(note 'out-1) {capture} (note 'out-2)");
    let after = match shape.transfer {
        Transfer::ReenterAfter => after_capture.as_str(),
        Transfer::EscapeThenReenterAfter | Transfer::AbortThenReenterAfter => {
            "(note 'out-1) (call/cc (lambda (c) (set! kt c))) (note 'out-2)"
        }
        Transfer::ResumeThenReenterAfter => {
            "(note 'out-1) (when (= n 1) (call/cc (lambda (c) (set! kt c)))) (note 'out-2)"
        }
        Transfer::AbortFromAfter | Transfer::ResumeFromAfter => {
            "(note 'out-1) (abort-current-continuation t 'ab) (note 'out-2)"
        }
        _ => out,
    };
    let core = positioned(shape, &extent(shape, before, body, after));
    match shape.transfer {
        Transfer::None => format!("{prelude}\n(define r {core})\n(list r (reverse log))"),
        Transfer::Escape => {
            format!("{prelude}\n(define r (call/cc (lambda (k) {core})))\n(list r (reverse log))")
        }
        Transfer::Reenter | Transfer::ReenterBefore | Transfer::ReenterAfter => format!(
            "{prelude}\n(define k #f)\n(define n 0)\n(define r {core})\n\
             (when (< n 1) (set! n 1) (k 'again))\n(list r (reverse log))"
        ),
        Transfer::Abort | Transfer::AbortFromBefore | Transfer::AbortFromAfter => format!(
            "{prelude}\n(define t (make-continuation-prompt-tag 'p))\n\
             (define r (call-with-continuation-prompt (lambda () {core}) t (lambda (v k) (list 'h v))))\n\
             (list r (reverse log))"
        ),
        Transfer::Resume | Transfer::ResumeFromBefore | Transfer::ResumeFromAfter => format!(
            "{prelude}\n(define t (make-continuation-prompt-tag 'p))\n(define k* #f)\n\
             (define cap (call-with-continuation-prompt (lambda () {core}) t (lambda (v k) (set! k* k) 'cap)))\n\
             (define r (k* 'resumed))\n(list r (reverse log))"
        ),
        Transfer::JumpThenReenterBefore => format!(
            "{prelude}\n(define k #f)\n(define kt #f)\n(define n 0)\n(define r {core})\n\
             (when (= n 0) (set! n 1) (k 'again))\n\
             (when (= n 1) (set! n 2) (kt 'thunk))\n(list r (reverse log))"
        ),
        Transfer::EscapeThenReenterAfter => format!(
            "{prelude}\n(define kt #f)\n(define n 0)\n\
             (define r (call/cc (lambda (k) {core})))\n\
             (when (= n 0) (set! n 1) (kt 'thunk))\n(list r (reverse log))"
        ),
        Transfer::AbortThenReenterAfter => format!(
            "{prelude}\n(define kt #f)\n(define n 0)\n\
             (define t (make-continuation-prompt-tag 'p))\n\
             (define r (call-with-continuation-prompt (lambda () {core}) t (lambda (v k) (list 'h v))))\n\
             (when (= n 0) (set! n 1) (kt 'thunk))\n(list r (reverse log))"
        ),
        Transfer::ResumeThenReenterAfter => format!(
            "{prelude}\n(define t (make-continuation-prompt-tag 'p))\n(define k* #f)\n\
             (define kt #f)\n(define n 0)\n\
             (define cap (call-with-continuation-prompt (lambda () {core}) t (lambda (v k) (set! k* k) 'cap)))\n\
             (set! n 1)\n(define r (k* 'resumed))\n\
             (when (= n 1) (set! n 2) (kt 'thunk))\n(list r (reverse log))"
        ),
        Transfer::ResumeThenReenterThunk => format!(
            "{prelude}\n(define t (make-continuation-prompt-tag 'p))\n(define k* #f)\n\
             (define kt #f)\n(define n 0)\n\
             (define cap (call-with-continuation-prompt (lambda () {core}) t (lambda (v k) (set! k* k) 'cap)))\n\
             (define r (k* 'resumed))\n\
             (when (< n 1) (set! n 1) (kt 'again))\n(list r (reverse log))"
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
    // Full jump: capture only during re-entry, then replay the pending jump.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::JumpThenReenterBefore,
            correct: "(again (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(again (in-1 in-2 out in-1 in-2 out in-2 out))", tw: "(again (in-1 in-2 out in-1 in-2 out in-2 out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::JumpThenReenterBefore,
            correct: "((w again) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "((w again) (in-1 in-2 out in-1 in-2 out in-2 out))", tw: "((w again) (in-1 in-2 out in-1 in-2 out in-2 out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::JumpThenReenterBefore,
            correct: "(again (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(again (in-1 in-2 out in-1 in-2 out in-2 out))", tw: "(again (in-1 in-2 out in-1 in-2 out in-2 out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::JumpThenReenterBefore,
            correct: "((w again) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "((w again) (in-1 in-2 out in-1 in-2 out in-2 out))", tw: "((w again) (in-1 in-2 out in-1 in-2 out in-2 out))",
            oracles: "chibi, gauche, guile", issue: "" },
    // Full escape: replay the exiting thunk without re-entering its extent.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::EscapeThenReenterAfter,
            correct: "(escaped (in out-1 out-2 out-2))", vm: "(escaped (in out-1 out-2 out-2))", tw: "(escaped (in out-1 out-2 out-2))",
            oracles: "gauche, guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::EscapeThenReenterAfter,
            correct: "(escaped (in out-1 out-2 out-2))", vm: "(escaped (in out-1 out-2 out-2))", tw: "(escaped (in out-1 out-2 out-2))",
            oracles: "gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::EscapeThenReenterAfter,
            correct: "(escaped (in out-1 out-2 out-2))", vm: "(escaped (in out-1 out-2 out-2))", tw: "(escaped (in out-1 out-2 out-2))",
            oracles: "gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::EscapeThenReenterAfter,
            correct: "(escaped (in out-1 out-2 out-2))", vm: "(escaped (in out-1 out-2 out-2))", tw: "(escaped (in out-1 out-2 out-2))",
            oracles: "gauche, guile", issue: "" },
    // Abort travel: replay the exiting thunk and still deliver the abort value.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::AbortThenReenterAfter,
            correct: "((h ab) (in out-1 out-2 out-2))", vm: "((h ab) (in out-1 out-2 out-2))", tw: "((h ab) (in out-1 out-2 out-2))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::AbortThenReenterAfter,
            correct: "((h ab) (in out-1 out-2 out-2))", vm: "((h ab) (in out-1 out-2 out-2))", tw: "((h ab) (in out-1 out-2 out-2))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::AbortThenReenterAfter,
            correct: "((h ab) (in out-1 out-2 out-2))", vm: "((h ab) (in out-1 out-2 out-2))", tw: "((h ab) (in out-1 out-2 out-2))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::AbortThenReenterAfter,
            correct: "((h ab) (in out-1 out-2 out-2))", vm: "((h ab) (in out-1 out-2 out-2))", tw: "((h ab) (in out-1 out-2 out-2))",
            oracles: "guile", issue: "" },
    // Resumed region: replay its after-thunk, preserving the resumed result.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::ResumeThenReenterAfter,
            correct: "((got resumed) (in out-1 out-2 in out-1 out-2 out-2))", vm: "((got resumed) (in out-1 out-2 in out-1 out-2 out-2))", tw: "((got resumed) (in out-1 out-2 in out-1 out-2 out-2))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::ResumeThenReenterAfter,
            correct: "((w (got resumed)) (in out-1 out-2 in out-1 out-2 out-2))", vm: "((w (got resumed)) (in out-1 out-2 in out-1 out-2 out-2))", tw: "((w (got resumed)) (in out-1 out-2 in out-1 out-2 out-2))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::ResumeThenReenterAfter,
            correct: "((got resumed) (in out-1 out-2 in out-1 out-2 out-2))", vm: "((got resumed) (in out-1 out-2 in out-1 out-2 out-2))", tw: "((got resumed) (in out-1 out-2 in out-1 out-2 out-2))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::ResumeThenReenterAfter,
            correct: "((w (got resumed)) (in out-1 out-2 in out-1 out-2 out-2))", vm: "((w (got resumed)) (in out-1 out-2 in out-1 out-2 out-2))", tw: "((w (got resumed)) (in out-1 out-2 in out-1 out-2 out-2))",
            oracles: "guile", issue: "" },
    // Abort from before: the extent has not been entered.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::AbortFromBefore,
            correct: "((h ab) (in-1))", vm: "((h ab) (in-1))", tw: "((h ab) (in-1))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::AbortFromBefore,
            correct: "((h ab) (in-1))", vm: "((h ab) (in-1))", tw: "((h ab) (in-1))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::AbortFromBefore,
            correct: "((h ab) (in-1))", vm: "((h ab) (in-1))", tw: "((h ab) (in-1))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::AbortFromBefore,
            correct: "((h ab) (in-1))", vm: "((h ab) (in-1))", tw: "((h ab) (in-1))",
            oracles: "guile, racket", issue: "" },
    // Abort from after: the extent has already been left.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::AbortFromAfter,
            correct: "((h ab) (in out-1))", vm: "((h ab) (in out-1))", tw: "((h ab) (in out-1))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::AbortFromAfter,
            correct: "((h ab) (in out-1))", vm: "((h ab) (in out-1))", tw: "((h ab) (in out-1))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::AbortFromAfter,
            correct: "((h ab) (in out-1))", vm: "((h ab) (in out-1))", tw: "((h ab) (in out-1))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::AbortFromAfter,
            correct: "((h ab) (in out-1))", vm: "((h ab) (in out-1))", tw: "((h ab) (in out-1))",
            oracles: "guile, racket", issue: "" },
    // Resume before: finish entry, then run the body and exit.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::ResumeFromBefore,
            correct: "(body (in-1 in-2 out))", vm: "(body (in-1 in-2 out))", tw: "(body (in-1 in-2 out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::ResumeFromBefore,
            correct: "((w body) (in-1 in-2 out))", vm: "((w body) (in-1 in-2 out))", tw: "((w body) (in-1 in-2 out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::ResumeFromBefore,
            correct: "(body (in-1 in-2 out))", vm: "(body (in-1 in-2 out))", tw: "(body (in-1 in-2 out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::ResumeFromBefore,
            correct: "((w body) (in-1 in-2 out))", vm: "((w body) (in-1 in-2 out))", tw: "((w body) (in-1 in-2 out))",
            oracles: "guile, racket", issue: "" },
    // Resume after: finish exit and return the saved body result.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::ResumeFromAfter,
            correct: "(body (in out-1 out-2))", vm: "(body (in out-1 out-2))", tw: "(body (in out-1 out-2))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::ResumeFromAfter,
            correct: "((w body) (in out-1 out-2))", vm: "((w body) (in out-1 out-2))", tw: "((w body) (in out-1 out-2))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::ResumeFromAfter,
            correct: "(body (in out-1 out-2))", vm: "(body (in out-1 out-2))", tw: "(body (in out-1 out-2))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::ResumeFromAfter,
            correct: "((w body) (in out-1 out-2))", vm: "((w body) (in out-1 out-2))", tw: "((w body) (in out-1 out-2))",
            oracles: "guile, racket", issue: "" },

    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::ReenterBefore,
            correct: "(body (in-1 in-2 out in-2 out))", vm: "(body (in-1 in-2 out in-2 out))", tw: "(body (in-1 in-2 out in-2 out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::ReenterBefore,
            correct: "((w body) (in-1 in-2 out in-2 out))", vm: "((w body) (in-1 in-2 out in-2 out))", tw: "((w body) (in-1 in-2 out in-2 out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::ReenterBefore,
            correct: "(body (in-1 in-2 out in-2 out))", vm: "(body (in-1 in-2 out in-2 out))", tw: "(body (in-1 in-2 out in-2 out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::ReenterBefore,
            correct: "((w body) (in-1 in-2 out in-2 out))", vm: "((w body) (in-1 in-2 out in-2 out))", tw: "((w body) (in-1 in-2 out in-2 out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::ReenterAfter,
            correct: "(body (in out-1 out-2 out-2))", vm: "(body (in out-1 out-2 out-2))", tw: "(body (in out-1 out-2 out-2))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::ReenterAfter,
            correct: "((w body) (in out-1 out-2 out-2))", vm: "((w body) (in out-1 out-2 out-2))", tw: "((w body) (in out-1 out-2 out-2))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::ReenterAfter,
            correct: "(body (in out-1 out-2 out-2))", vm: "(body (in out-1 out-2 out-2))", tw: "(body (in out-1 out-2 out-2))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::ReenterAfter,
            correct: "((w body) (in out-1 out-2 out-2))", vm: "((w body) (in out-1 out-2 out-2))", tw: "((w body) (in out-1 out-2 out-2))",
            oracles: "chibi, gauche, guile", issue: "" },

    // ---- no transfer: the extent's own bookkeeping ---------------------
    // The baseline. Every other row adds a transfer to this one, so a change that
    // moves these four moves everything and is about the extent, not the transfer.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::None,
            correct: "(body (in out))", vm: "(body (in out))", tw: "(body (in out))",
            oracles: "chibi, gauche, guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::None,
            correct: "((w body) (in out))", vm: "((w body) (in out))", tw: "((w body) (in out))",
            oracles: "chibi, gauche, guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::None,
            correct: "(body (in out))", vm: "(body (in out))", tw: "(body (in out))",
            oracles: "chibi, gauche, guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::None,
            correct: "((w body) (in out))", vm: "((w body) (in out))", tw: "((w body) (in out))",
            oracles: "chibi, gauche, guile, racket", issue: "" },

    // ---- escape: a jump OUT of the body --------------------------------
    // One entry, one exit, and the value the jump carries.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Escape,
            correct: "(escaped (in out))", vm: "(escaped (in out))", tw: "(escaped (in out))",
            oracles: "chibi, gauche, guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Escape,
            correct: "(escaped (in out))", vm: "(escaped (in out))", tw: "(escaped (in out))",
            oracles: "chibi, gauche, guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Escape,
            correct: "(escaped (in out))", vm: "(escaped (in out))", tw: "(escaped (in out))",
            oracles: "chibi, gauche, guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Escape,
            correct: "(escaped (in out))", vm: "(escaped (in out))", tw: "(escaped (in out))",
            oracles: "chibi, gauche, guile, racket", issue: "" },

    // ---- reenter: a jump back IN ---------------------------------------
    // Both thunks run twice. Racket cannot answer these: re-entering re-executes a
    // module-level `define`, which its module system forbids — a rule about
    // modules, not about control flow.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Reenter,
            correct: "(again (in out in out))", vm: "(again (in out in out))", tw: "(again (in out in out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Reenter,
            correct: "((w again) (in out in out))", vm: "((w again) (in out in out))", tw: "((w again) (in out in out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Reenter,
            correct: "(again (in out in out))", vm: "(again (in out in out))", tw: "(again (in out in out))",
            oracles: "chibi, gauche, guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Reenter,
            correct: "((w again) (in out in out))", vm: "((w again) (in out in out))", tw: "((w again) (in out in out))",
            oracles: "chibi, gauche, guile", issue: "" },

    // ---- abort: out to a prompt ----------------------------------------
    // PR #158 fixed a `no active frame` panic on exactly one of these four:
    // the *value* form in non-tail position. Head position was never affected,
    // and the tail spelling pops its frame first and survived — which is why
    // both axes are here rather than either alone.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: "((h ab) (in out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: "((h ab) (in out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: "((h ab) (in out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: "((h ab) (in out))",
            oracles: "guile, racket", issue: "" },

    // ---- resume: invoke the composable continuation --------------------
    // Issue #160's shape: the value has to reach the hole *and* come back out.
    // The captured extent is re-entered, so the log doubles.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Resume,
            correct: "((got resumed) (in out in out))", vm: "((got resumed) (in out in out))", tw: "((got resumed) (in out in out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Resume,
            correct: "((w (got resumed)) (in out in out))", vm: "((w (got resumed)) (in out in out))", tw: "((w (got resumed)) (in out in out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Resume,
            correct: "((got resumed) (in out in out))", vm: "((got resumed) (in out in out))", tw: "((got resumed) (in out in out))",
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Resume,
            correct: "((w (got resumed)) (in out in out))", vm: "((w (got resumed)) (in out in out))", tw: "((w (got resumed)) (in out in out))",
            oracles: "guile, racket", issue: "" },

    // ---- resume, then re-enter a continuation captured in a thunk ------
    // Issue #167, fixed. It was on all four spellings — one defect no spelling
    // avoided, which is what the enumeration added to the single filed repro.
    // The resumed value was lost (`()` for `(got resumed)`) and the extent left
    // unbalanced, the last `out` never running, because `invoke_delimited` ran
    // these thunks on a nested Rust loop and the re-entry had no pc to come
    // back to. Each now runs under a `ResumeComposableInvoke` stub — *not*
    // through `step_wind_jump`, which is what fixed the abort's thunks in #165
    // and which is wrong here: see `install_thunk_handlers`.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))", tw: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))", tw: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))", tw: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))",
            oracles: "guile", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))", tw: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))",
            oracles: "guile", issue: "" },
];

/// Write every program out, once per oracle, **ready to run**.
///
/// Ignored by default: it asserts nothing, it exists so the table can be
/// re-*measured* rather than re-derived. The programs an oracle is fed must be
/// the programs this test runs, and the only way to guarantee that is to take
/// them from the same generator.
///
/// Each file is the program plus two things it needs to be a script: the
/// oracle's import or `#lang` line, and a `write` around the final expression,
/// which every implementation here evaluates silently otherwise. For the
/// prompt-using oracles it also carries a shim spelling Patina's prompt API in
/// terms of theirs — Guile's handler takes `(k v)` where Patina's takes
/// `(v k)`, and Racket's abort passes values only, so the continuation has to
/// be captured and carried. **The program text itself is never edited**: a
/// matrix whose oracle ran a different program would be measuring the
/// transcription.
///
/// ```text
/// CONTROL_FLOW_MATRIX_DUMP=/tmp/cfm cargo test -p patina-tests \
///     --test control_flow_matrix -- --ignored dump_programs
/// for f in /tmp/cfm/chibi/*.scm;  do chibi-scheme "$f"; done
/// for f in /tmp/cfm/gauche/*.scm; do gosh -r7 "$f"; done
/// for f in /tmp/cfm/guile/*.scm;  do guile --r7rs --no-auto-compile "$f"; done
/// for f in /tmp/cfm/racket/*.rkt; do racket "$f"; done
/// ```
///
/// chibi and Gauche have no tagged prompt API, so their directories hold only
/// the shapes that do not need one.
#[test]
#[ignore]
fn dump_programs() {
    /// What each oracle needs in front of the program, and the file extension
    /// its runner expects.
    const ORACLES: &[(&str, &str, &str)] = &[
        ("patina", "scm", "(import (scheme base) (scheme write))\n"),
        ("chibi", "scm", "(import (scheme base) (scheme write))\n"),
        ("gauche", "scm", "(import (scheme base) (scheme write))\n"),
        (
            "guile",
            "scm",
            "(import (scheme base) (scheme write) (ice-9 control))\n\
             (define (make-continuation-prompt-tag . n) (apply make-prompt-tag n))\n\
             (define (call-with-continuation-prompt body tag handler)\n\
             \x20 (call-with-prompt tag body (lambda (k v) (handler v k))))\n\
             (define (abort-current-continuation tag . vals) (apply abort-to-prompt tag vals))\n",
        ),
        (
            "racket",
            "rkt",
            "#lang racket/base\n\
             (require (only-in racket/base [abort-current-continuation racket:abort]))\n\
             (define (abort-current-continuation tag v)\n\
             \x20 (call-with-composable-continuation (lambda (k) (racket:abort tag v k)) tag))\n",
        ),
    ];
    let dir = std::env::var("CONTROL_FLOW_MATRIX_DUMP")
        .expect("set CONTROL_FLOW_MATRIX_DUMP to a directory");
    for (oracle, ext, prelude) in ORACLES {
        let sub = format!("{dir}/{oracle}");
        std::fs::create_dir_all(&sub).expect("create dump directory");
        for shape in MATRIX {
            // chibi and Gauche cannot express a tagged prompt at all, so a
            // file for them would only fail in a way that says nothing.
            if shape.transfer.needs_prompts() && matches!(*oracle, "chibi" | "gauche") {
                continue;
            }
            let code = program(shape);
            let (body, last) = code
                .rsplit_once('\n')
                .expect("every program has more than one line");
            let path = format!("{sub}/{}.{ext}", shape.name().replace('/', "_"));
            std::fs::write(&path, format!("{prelude}{body}\n(write {last})(newline)\n"))
                .expect("write program");
        }
    }
}

/// Run one shape on one backend, rendering a rejection as [`ERROR`].
fn answer(code: &str, vm: bool) -> String {
    let ok = if vm {
        try_eval_program_vm(code)
    } else {
        try_eval_program_tree_walker(code)
    };
    ok.unwrap_or_else(|e| {
        let first = e.lines().next().unwrap_or("").trim();
        format!("{ERROR}: {first}")
    })
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
                moved.push(common::describe_move(
                    &shape.name(),
                    backend,
                    recorded,
                    &got,
                    shape.correct,
                    &code,
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
/// backends through every defect this matrix exists for. These numbers can
/// only move deliberately, and there is one **per backend** — a tree-walker
/// regression has somewhere to be recorded, rather than colliding with an
/// assertion that it always matches.
#[test]
fn the_defect_count_is_what_it_says() {
    let wrong = |pick: fn(&Shape) -> &'static str, prompts: bool| {
        MATRIX
            .iter()
            .filter(|s| s.transfer.needs_prompts() == prompts && pick(s) != s.correct)
            .map(|s| s.name())
            .collect::<Vec<_>>()
    };
    let vm: Vec<String> = wrong(|s| s.vm, true)
        .into_iter()
        .chain(wrong(|s| s.vm, false))
        .collect();
    // Zero. It was four — issue #167, one defect on all four spellings — until
    // the composable invoke's re-entry thunks became frames. A row going red
    // now is a regression on a shape the reference implementations agree
    // about, whichever direction it moved.
    assert_eq!(vm.len(), 0, "VM shapes wrong: {vm:?}");

    // The tree-walker: zero as well. Its twelve prompt rows recorded
    // `UNSUPPORTED` until 2026-09-04 — an absent feature, not a wrong answer
    // — and went red together when the API landed, as the header promised.
    let tw: Vec<String> = wrong(|s| s.tw, true)
        .into_iter()
        .chain(wrong(|s| s.tw, false))
        .collect();
    assert_eq!(tw.len(), 0, "tree-walker shapes wrong: {tw:?}");

    // Every wrong row names its issue, so the table doubles as the index from
    // defect to surface count; and every row names the implementations behind
    // its `correct` value, so no row borrows another's confidence.
    for shape in MATRIX {
        let wrong_somewhere = shape.vm != shape.correct || shape.tw != shape.correct;
        assert_eq!(
            wrong_somewhere,
            !shape.issue.is_empty(),
            "{}: `issue` must be set exactly on rows a backend gets wrong",
            shape.name()
        );
        assert!(
            !shape.oracles.is_empty(),
            "{}: every row records which implementations answer its `correct`",
            shape.name()
        );
    }
}

/// The space is a full cross product, with no row written twice.
///
/// Derived from the axis lists rather than from a literal count: the header
/// recommends adding a transfer, and against a hardcoded `2 * 2 * 5` that
/// leaves four points silently unscored — the one failure mode this test is
/// for. Naming the missing rows is the point; a bare count would say only
/// that something is off.
#[test]
fn the_matrix_is_a_complete_cross_product() {
    let mut expected: Vec<String> = Vec::new();
    for extent in EXTENTS {
        for position in POSITIONS {
            for transfer in TRANSFERS {
                expected.push(format!(
                    "{}/{}/{}",
                    extent.name(),
                    position.name(),
                    transfer.name()
                ));
            }
        }
    }
    let mut present: Vec<String> = MATRIX.iter().map(|s| s.name()).collect();
    let total = present.len();
    present.sort();
    present.dedup();
    assert_eq!(present.len(), total, "duplicate rows in MATRIX");

    let missing: Vec<&String> = expected.iter().filter(|n| !present.contains(n)).collect();
    let extra: Vec<&String> = present.iter().filter(|n| !expected.contains(n)).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "MATRIX is not the cross product of the axes.\n  missing: {missing:?}\n  extra:   {extra:?}"
    );
}
