//! Which sites decide "is this callable", and what they decide.
//!
//! This file exists because prose about those sites kept being wrong. Reviewing
//! the `procedure?`-on-parameters fix turned up three claims in its own commit
//! message that no test could have contradicted: that `dynamic-wind` validates
//! its arguments (it does not), that a certain count of call sites "became
//! correct together" (it was counting grep hits, not decisions), and that
//! `Heap::is_procedure` had become the single source of truth for callability
//! (it has not). Each was a statement about observable behaviour, so each is
//! pinned below.
//!
//! The rule these encode: a claim about *which* check runs is testable by
//! ordering or by what is accepted, without depending on error text — error
//! messages are not a stable interface, and two sites here share one message
//! verbatim.
//!
//! A second rule, learned the same way: "the other backend does X" is not a
//! reason to believe X — the convergence test below was checked against chibi,
//! Gauche and Chez before the tree-walker was changed to agree with the VM.
//!
//! A third: a defect class cannot be enumerated by grep. The first sweep of
//! `cps_eval/application.rs` matched `return Err(…)` and missed three
//! catchable errors that reach Rust through `?`.
//!
//! `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 carries the history.
//!
//! # What is left here, and what moved
//!
//! Most of this file is now `tests/scheme/callability.scm`, run by
//! `scheme_suite.rs` on both backends (#193 Phase 0). What stayed is what a
//! `.scm` file cannot express:
//!
//! - a row whose two backends give **different values** — expressible only
//!   once a backend names itself to `cond-expand`, which is not yet built;
//! - rows deliberately asserted on **one backend only**, for a reason that is
//!   not a divergence about the answer;
//! - `assert_program_eval_error_at`, which asserts *which stage* rejected the
//!   program. Not observable from inside Scheme.
//!
//! That split is the line #193 predicted the migration would fall on: what is
//! about the *language* went to Scheme, what is about the *implementation*
//! stayed in Rust.

mod common;
use common::{
    ErrorClass, assert_program_eval_error_at, assert_program_eval_to, eval_program_tree_walker,
    eval_program_vm,
};

/// The known limit, stated as behaviour: `procedure?` is *wider* than what the
/// sites requiring a procedure will accept.
///
/// A continuation answers `procedure?` with `#t`, yet `make-parameter` rejects
/// it as a converter — because callability is spelled two ways.
/// `Heap::is_procedure` covers closures, primitives, VM continuation refs and
/// (since the parameter fix) parameter objects, while tree-walker continuations
/// live in `Heap::is_continuation`. `Heap::is_callable` is the union, and the
/// sites that mean "any callable" now use it; the four that deliberately ask
/// the narrower question still reject a continuation. Folding `Continuation`
/// into `is_procedure` would widen those four — a behaviour change wanting its
/// own tests.
///
/// **Decided 2026-08-25**, when the VM's generic call path learned to invoke
/// a continuation (so `with-exception-handler` could take one, R7RS's idiom
/// for capturing a raised object): a continuation *is* a procedure for
/// `make-parameter` too — chibi and Gauche apply it to the initial value and
/// escape through it, answering `1`. The VM does the same, through `call_any`.
/// The tree-walker still rejects it, and deliberately so: its converter
/// runs as a direct-mode primitive callback, from which a continuation
/// cannot be invoked at all (PRD §6, "two continuation defects around
/// primitive callbacks"), so the clean rejection is the better of its two
/// answers until that is fixed. Not `assert_divergence` — the tree-walker
/// returns a value, not a failure.
#[test]
fn test_procedure_p_is_wider_than_the_sites_that_require_a_procedure() {
    assert_program_eval_to("(call/cc (lambda (k) (procedure? k)))", "#t");
    const CONVERTER: &str = r#"(call/cc (lambda (k)
             (guard (e (#t 'rejected)) (make-parameter 1 k) 'accepted)))"#;
    assert_eq!(
        eval_program_vm(CONVERTER),
        "1",
        "the VM matches chibi and Gauche; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_tree_walker(CONVERTER),
        "rejected",
        "\n[tree-walker] NO LONGER DIVERGES — a continuation converter is applied.\n\
         Replace both assertions with assert_program_eval_to(CONVERTER, \"1\") and \
         widen make-parameter's check to is_callable."
    );
    // The same site accepts an ordinary procedure, so the rejection above is
    // about which spelling of "callable" it uses, not about converters.
    assert_program_eval_to(
        "(guard (e (#t 'rejected)) (make-parameter 1 (lambda (x) x)) 'accepted)",
        "accepted",
    );
}

/// The rest of the VM's frameless call sites take a control primitive too.
///
/// `call_any` is one dispatcher with several callers, and the neighbouring
/// tests exercise the two whose answers both backends agree on. These are the
/// remainder, one program apiece, because "the same function serves them all"
/// is the kind of claim this file exists to distrust: the prompt body only
/// *became* a caller in issue #179, and inherited the hole in silence.
///
/// **VM-only assertions, and not because the tree-walker disagrees about the
/// answer.** Each of these names a control primitive in value position, which
/// the tree-walker resolves through a registry binding that is not there —
/// the hole `backend_divergence.rs::callcc_bound_with_define` and its two
/// neighbours already pin, still Q2 part 1's to fix. Pinning three more rows
/// of that one family here would just be three more things to collapse when
/// it lands.
///
/// Every row was measured against `main` at `30e0bd6` before the fix: each was
/// `Undefined variable: patina.internal.control/…`, or the `Internal error`
/// that name lookup becomes when it is a primitive callback that fails.
///
/// **A zero-argument call site cannot appear here, and that is not the same
/// as its being unaffected** — an earlier draft of this comment said "those
/// sites could never show the defect", which is false, and
/// [`a_wind_thunk_reaches_the_probe_it_cannot_satisfy`] below is the program
/// that falsifies it. A wind thunk and a `call-with-values` producer take no
/// arguments, and every control primitive but `values` requires at least one,
/// so none of them can *succeed* there. Reaching the probe and satisfying it
/// are different questions, and the arity is only an answer to the second.
#[test]
fn every_frameless_call_site_takes_a_control_primitive() {
    // `call/cc`'s own procedure argument, given `call/cc`.
    assert_eq!(eval_program_vm("(procedure? (call/cc call/cc))"), "#t");
    // A parameter converter, the one caller that must have its value
    // synchronously, so it runs a nested dispatch loop for a callee that
    // pushed a frame. The converter runs on the initial value too (R7RS 4.2.6).
    assert_eq!(
        eval_program_vm("(define q (make-parameter (lambda (k) 5) call/cc))\n(q)"),
        "5"
    );
    // A higher-order primitive's callback, which re-enters the VM from Rust:
    // `assoc`'s and `member`'s comparator. `(apply + '(1 2))` is 3, so the
    // first entry matches.
    assert_eq!(
        eval_program_vm("(assoc + (list (list '(1 2))) apply)"),
        "((1 2))"
    );
    assert_eq!(eval_program_vm("(member + (list '(1 2)) apply)"), "((1 2))");
}

/// A wind thunk does reach the probe — it just cannot satisfy it.
///
/// The row that makes the point the neighbour above gets wrong. `apply` as a
/// jump's `after` thunk goes through `push_wind_step` → `call_any` and is
/// called with **no arguments**, so it fails either way; *how* it fails is the
/// whole difference, and it is the same difference every other row in this
/// file shows:
///
/// ```text
///   main 30e0bd6 => Undefined variable: patina.internal.control/apply
///   with #186    => wrong number of arguments: expected at least 2, got 0
/// ```
///
/// The first is the name never resolving. The second is `apply` being reached,
/// recognised, and told it was called wrongly — which is what the tree-walker
/// has always said here, so the two backends now agree on the diagnosis and
/// not merely on the fact of failure.
///
/// The message is the assertion because nothing else can be: `values` is the
/// only control primitive a 0-argument call site can call successfully, and it
/// is in the registry, so it was found by the old probe too. That is the one
/// shape where this file's usual rule — never assert on error text — has no
/// alternative to fall back on, and the diagnosis is the behaviour under test.
#[test]
fn a_wind_thunk_reaches_the_probe_it_cannot_satisfy() {
    assert_program_eval_error_at(
        "(define k #f)\n\
         (define done #f)\n\
         (define v (call/cc (lambda (c) (set! k c) 0)))\n\
         (if (not done)\n\
         \x20   (begin (set! done #t)\n\
         \x20          (dynamic-wind (lambda () 'b) (lambda () (k 1)) apply)))\n\
         v",
        ErrorClass::AtRuntime,
        ErrorClass::AtRuntime,
        "number of arguments: expected at least 2, got 0",
    );
}
