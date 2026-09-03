//! A generated matrix of control-transfer shapes, scored against reference
//! implementations.
//!
//! Every defect this area has had was found one at a time, by hand-writing a
//! program that turned out to answer wrong — issues #157, #159, #160, #162,
//! #163, #165 and #167, closed by PRs #158, #161, #164 and #166 — while the
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
//! ```text
//!   none, escape           4   chibi, gauche, guile, racket
//!   reenter                3   Racket forbids re-executing a module-level
//!                              `define`, which is a module-system rule
//!   abort, resume          2   guile, racket — chibi and Gauche have no
//!                              tagged prompt API at all
//!   resume+thunk-reenter   1   guile
//! ```
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
//! On its first run, the four `resume+thunk-reenter` rows. That is issue #167,
//! which was already filed from a single repro — so the enumeration found no
//! *new* defect. What it added is that no spelling of the program avoids it:
//! both extent forms and both positions fail identically, which the one repro
//! could not say and which is what a fix has to hold against.
//!
//! # What the tree-walker does here
//!
//! It has no prompt API at all, so the twelve prompt rows record
//! [`UNSUPPORTED`] — a classification, not the error's prose. That is an
//! absent feature, not a wrong answer, and
//! [`the_defect_count_is_what_it_says`] keeps the two apart: a count of wrong
//! rows per backend, plus an assertion that every prompt row says exactly
//! "unsupported". When the tree-walker grows prompts those twelve go red
//! together, which is a request to measure them rather than a failure.
//!
//! These twelve rows and the four #167 rows are 16 pinned-known-wrong
//! assertions that `assert_divergence` does not know about. `common/mod.rs`
//! names this file as the second inventory for that reason.
//!
//! # Adding axes
//!
//! The axes here are the ones the defect history names. Worth adding next,
//! roughly in order of value:
//!
//! - **where inside the extent a continuation is captured** — the body, the
//!   `before` thunk, the `after` thunk. Only
//!   [`Transfer::ResumeThenReenterThunk`] reaches into a thunk today, and that
//!   one axis produced #157, #159, #165 and #167. As a separate `Capture` axis
//!   it would multiply these rows by three; as one more `Transfer` it stays
//!   cheap — and [`the_matrix_is_a_complete_cross_product`] will name the rows
//!   the addition leaves unwritten.
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

/// The tree-walker has no prompt API, so every shape needing one fails there
/// before running a line of the program.
///
/// A **classification**, not the error's prose. `common/mod.rs` says why text
/// is never compared — "pinning prose would turn every message improvement
/// into a test break" — and its `ErrorClass` is too coarse to help here, being
/// two buckets that both say "at run time". So [`answer`] recognises this one
/// failure by the primitive's own name, which survives rewording `Undefined
/// variable` and renaming the `patina.internal.control` path alike.
const UNSUPPORTED: &str = "<unsupported: no prompt API>";

/// How any other rejection is rendered, so a row that starts failing says
/// which. Composed into messages rather than repeated as a literal.
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
            Transfer::None => "none",
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
    Transfer::Abort,
    Transfer::Resume,
    Transfer::ResumeThenReenterThunk,
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
    /// Per row, not per file: the eight prompt-free shapes have four, the
    /// re-entry ones three (Racket cannot re-execute a module-level `define`),
    /// and the prompt shapes two or one. A single number across the table
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
        Transfer::None => "'body",
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
        Transfer::None => format!("{prelude}\n(define r {core})\n(list r (reverse log))"),
        Transfer::Escape => {
            format!("{prelude}\n(define r (call/cc (lambda (k) {core})))\n(list r (reverse log))")
        }
        Transfer::Reenter => format!(
            "{prelude}\n(define k #f)\n(define n 0)\n(define r {core})\n\
             (when (< n 1) (set! n 1) (k 'again))\n(list r (reverse log))"
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
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: UNSUPPORTED,
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: UNSUPPORTED,
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: UNSUPPORTED,
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Abort,
            correct: "((h ab) (in out))", vm: "((h ab) (in out))", tw: UNSUPPORTED,
            oracles: "guile, racket", issue: "" },

    // ---- resume: invoke the composable continuation --------------------
    // Issue #160's shape: the value has to reach the hole *and* come back out.
    // The captured extent is re-entered, so the log doubles.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::Resume,
            correct: "((got resumed) (in out in out))", vm: "((got resumed) (in out in out))", tw: UNSUPPORTED,
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::Resume,
            correct: "((w (got resumed)) (in out in out))", vm: "((w (got resumed)) (in out in out))", tw: UNSUPPORTED,
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::Resume,
            correct: "((got resumed) (in out in out))", vm: "((got resumed) (in out in out))", tw: UNSUPPORTED,
            oracles: "guile, racket", issue: "" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::Resume,
            correct: "((w (got resumed)) (in out in out))", vm: "((w (got resumed)) (in out in out))", tw: UNSUPPORTED,
            oracles: "guile, racket", issue: "" },

    // ---- resume, then re-enter a continuation captured in a thunk ------
    // Issue #167, on all four spellings — not four defects, one defect that no
    // spelling avoids. The resumed value is lost (`()` for `(got resumed)`) and
    // the extent is left unbalanced, the last `out` never running.
    // `invoke_delimited` runs these thunks on a nested Rust loop, so the re-entry
    // has no pc to come back to; the issue records why routing them through
    // `step_wind_jump`, which is what fixed the abort's thunks in #165, is *not*
    // the fix here.
    Shape { extent: Extent::Head, position: Position::Tail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(() (in-1 in-2 out in-1 in-2 out in-2))", tw: UNSUPPORTED,
            oracles: "guile", issue: "#167" },
    Shape { extent: Extent::Head, position: Position::NonTail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(() (in-1 in-2 out in-1 in-2 out in-2))", tw: UNSUPPORTED,
            oracles: "guile", issue: "#167" },
    Shape { extent: Extent::Value, position: Position::Tail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((got resumed) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(() (in-1 in-2 out in-1 in-2 out in-2))", tw: UNSUPPORTED,
            oracles: "guile", issue: "#167" },
    Shape { extent: Extent::Value, position: Position::NonTail, transfer: Transfer::ResumeThenReenterThunk,
            correct: "((w (got resumed)) (in-1 in-2 out in-1 in-2 out in-2 out))", vm: "(() (in-1 in-2 out in-1 in-2 out in-2))", tw: UNSUPPORTED,
            oracles: "guile", issue: "#167" },
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
    ok.unwrap_or_else(|e| {
        let first = e.lines().next().unwrap_or("").trim();
        if first.contains("call-with-continuation-prompt") {
            UNSUPPORTED.to_string()
        } else {
            format!("{ERROR}: {first}")
        }
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
    // Four: issue #167. They are four spellings of one defect, not four
    // defects — the enumeration's value here is showing that no spelling
    // avoids it, which a single filed repro could not say.
    assert_eq!(vm.len(), 4, "VM shapes wrong: {vm:?}");
    assert!(
        vm.iter().all(|n| n.ends_with("resume+thunk-reenter")),
        "the VM's wrong rows should all be #167's: {vm:?}"
    );

    // The tree-walker: zero wrong among the shapes it can run.
    let tw = wrong(|s| s.tw, false);
    assert_eq!(tw.len(), 0, "tree-walker shapes wrong: {tw:?}");

    // …and the shapes it cannot run say exactly that. When it grows a prompt
    // API these twelve go red together, which is a request to measure them.
    for shape in MATRIX.iter().filter(|s| s.transfer.needs_prompts()) {
        assert_eq!(
            shape.tw,
            UNSUPPORTED,
            "{}: the tree-walker has no prompt API, so this row records that",
            shape.name()
        );
    }

    // Every wrong row names its issue, so the table doubles as the index from
    // defect to surface count; and every row names the implementations behind
    // its `correct` value, so no row borrows another's confidence.
    for shape in MATRIX {
        let wrong_somewhere = shape.vm != shape.correct
            || (!shape.transfer.needs_prompts() && shape.tw != shape.correct);
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
