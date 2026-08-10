//! Known behavioural divergences between the tree-walker and the VM.
//!
//! Every other test file in this crate holds both backends to the *same*
//! expectation. This file is the exception list: one test per behaviour where
//! they currently disagree, each pinning what both backends actually do today.
//!
//! **These tests are designed to fail when the bug is fixed.** Each one asserts
//! the working backend's correct answer *and* that the other backend still
//! errors. Repairing the broken side turns the second assertion red, which
//! forces whoever fixes it to collapse the test into a plain
//! `assert_program_eval_to` covering both backends. A quarantine that does not
//! retire itself becomes a permanent excuse.
//!
//! Source: `PRD/TRACK_Q_QUALITY_PRD.md` §1.2, re-measured on this branch at
//! `2d4ce29` (2026-08-10). Two rows of that table have since drifted — see
//! `apply_values_agrees` and `apply_callcc_fails_on_both` below.
//!
//! Shared root cause (§1.2): R7RS §6.10 makes `call/cc`, `dynamic-wind`,
//! `values` and `with-exception-handler` ordinary procedures, but both backends
//! resolve them by name at the *call site*, and the registry binding behind the
//! name is missing or a stub. Every one of these works when called directly,
//! which is why the 1226/1226 chibi suite never catches it — that suite never
//! takes one of them as a value. Track Q Q2 is the fix.

mod common;
use common::*;

// ─── call/cc in value position ───────────────────────────────────────────────

/// DIVERGENCE: VM `1`, tree-walker `Undefined variable:
/// patina.internal.control/call/cc`. Fixed by Q2 part 1 (real registry binding).
#[test]
fn callcc_bound_with_define() {
    const CODE: &str = "(define f call/cc) (f (lambda (k) 1))";
    assert_program_eval_to_on(On::Vm, CODE, "1");
    assert_program_eval_error_on(On::TreeWalker, CODE);
}

/// DIVERGENCE: VM `(6)`, tree-walker undefined-variable. Same root cause as
/// [`callcc_bound_with_define`]; kept separate because passing a control op
/// *through a higher-order procedure* is the shape real code hits (SRFI 1).
#[test]
fn callcc_passed_to_higher_order_procedure() {
    const CODE: &str = "(map (lambda (f) (f (lambda (k) 6))) (list call/cc))";
    assert_program_eval_to_on(On::Vm, CODE, "(6)");
    assert_program_eval_error_on(On::TreeWalker, CODE);
}

/// NOT a divergence — both backends fail identically, so the differential
/// harness cannot see it. It is still an R7RS conformance gap: this must
/// evaluate to `1`. Recorded here so Q2 does not mistake backend *agreement*
/// for correctness.
#[test]
fn apply_callcc_fails_on_both() {
    assert_program_eval_error("(apply call/cc (list (lambda (k) 1)))");
}

// ─── dynamic-wind via apply ──────────────────────────────────────────────────

/// DIVERGENCE, opposite direction: tree-walker `2`, VM `Undefined variable:
/// patina.internal.control/dynamic-wind`. The two backends' holes are
/// complementary — the signature of never having run a differential test.
#[test]
fn apply_dynamic_wind() {
    const CODE: &str = r#"
        (define r '())
        (apply dynamic-wind
               (list (lambda () (set! r 1)) (lambda () 2) (lambda () (set! r 3))))
    "#;
    assert_program_eval_to_on(On::TreeWalker, CODE, "2");
    assert_program_eval_error_on(On::Vm, CODE);
}

// ─── with-exception-handler via apply ────────────────────────────────────────

/// DIVERGENCE: tree-walker `43`, VM hits the registered-but-stubbed primitive
/// whose body is `InternalError("with-exception-handler: not yet implemented")`
/// (`patina-primitives/src/primitives/exceptions.rs`). Q2 part 2 deletes or
/// implements that stub — a registered primitive that unconditionally errors is
/// worse than an unregistered one, because it turns a clean unbound-variable
/// error into an internal error and advertises support in the export list.
#[test]
fn apply_with_exception_handler() {
    const CODE: &str = r#"
        (apply with-exception-handler
               (list (lambda (e) 43) (lambda () (raise-continuable 'x))))
    "#;
    assert_program_eval_to_on(On::TreeWalker, CODE, "43");
    assert_program_eval_error_on(On::Vm, CODE);
}

// ─── Already converged ───────────────────────────────────────────────────────

/// §1.2 recorded this as a VM failure (`Wrong number of arguments: expected 1,
/// got 2`) at `7a6a797`. Both backends return `7` as of `2d4ce29`, so it is a
/// plain both-backends test now — kept here as the regression guard for a row
/// that was fixed without anyone noticing.
#[test]
fn apply_values_agrees() {
    assert_program_eval_to("(apply values (list 7))", "7");
}
