//! The registry of known behavioural divergences between the tree-walker and
//! the VM.
//!
//! Every other test file in this crate holds both backends to the *same*
//! expectation. This is the exception list, and `assert_divergence` is the only
//! way onto it: each call pins the working backend's answer, requires the other
//! to still fail, and names the document tracking the bug.
//!
//! **These tests are designed to fail when the bug is fixed** — repairing the
//! broken backend trips the second assertion, and the panic message says to
//! collapse the call into a plain `assert_program_eval_to`. A quarantine that
//! does not retire itself becomes a permanent excuse.
//!
//! Two tests at the end are *not* divergences: one records a gap both backends
//! share (so the differential harness cannot see it) and one guards a row that
//! has already converged. They live here because this is where someone working
//! the divergence list will look for them.
//!
//! Sources: `PRD/TRACK_Q_QUALITY_PRD.md` §1.2, re-measured at `2d4ce29`
//! (2026-08-10), `PRD/bugs/TREE_WALKER_CALLCC_MULTI_VALUES.md`, and
//! `PRD/AUDIT_2026_08_10_PRD.md` B3 (measured 2026-08-10).
//!
//! Shared root cause of the §1.2 cluster: R7RS §6.10 makes `call/cc`,
//! `dynamic-wind`, `values` and `with-exception-handler` ordinary procedures,
//! but both backends resolve them by name at the *call site*, and the registry
//! binding behind the name is missing or a stub. Every one works when called
//! directly, which is why the 1226/1226 chibi suite never catches it — that
//! suite never takes one of them as a value. Track Q Q2 is the fix.

mod common;
use common::*;

const CONTROL_OPS: &str = "PRD/TRACK_Q_QUALITY_PRD.md §1.2";
const CALLCC_MULTI_VALUES: &str = "PRD/bugs/TREE_WALKER_CALLCC_MULTI_VALUES.md";
const HANDLER_REENTRY: &str = "PRD/AUDIT_2026_08_10_PRD.md B3";

// ─── call/cc in value position (Track Q §1.2) ────────────────────────────────

/// Tree-walker: `Undefined variable: patina.internal.control/call/cc`.
/// Fixed by Q2 part 1 — a real registry binding behind the name.
#[test]
fn callcc_bound_with_define() {
    assert_divergence(
        "(define f call/cc) (f (lambda (k) 1))",
        On::Vm,
        "1",
        CONTROL_OPS,
    );
}

/// Same root cause as [`callcc_bound_with_define`], kept separate because
/// passing a control op *through a higher-order procedure* is the shape real
/// code hits (SRFI 1).
#[test]
fn callcc_passed_to_higher_order_procedure() {
    assert_divergence(
        "(map (lambda (f) (f (lambda (k) 6))) (list call/cc))",
        On::Vm,
        "(6)",
        CONTROL_OPS,
    );
}

// ─── apply on control ops (Track Q §1.2) ─────────────────────────────────────

/// The opposite direction: the VM reports `Undefined variable:
/// patina.internal.control/dynamic-wind`. The two backends' holes are
/// complementary — the signature of never having run a differential test.
#[test]
fn apply_dynamic_wind() {
    assert_divergence(
        r#"
        (define r '())
        (apply dynamic-wind
               (list (lambda () (set! r 1)) (lambda () 2) (lambda () (set! r 3))))
        "#,
        On::TreeWalker,
        "2",
        CONTROL_OPS,
    );
}

/// The VM hits the registered-but-stubbed primitive whose body is
/// `InternalError("with-exception-handler: not yet implemented")`
/// (`patina-primitives/src/primitives/exceptions.rs`). Q2 part 2 deletes or
/// implements that stub: a registered primitive that unconditionally errors is
/// worse than an unregistered one, because it turns a clean unbound-variable
/// error into an internal error and advertises support in the export list.
#[test]
fn apply_with_exception_handler() {
    assert_divergence(
        r#"
        (apply with-exception-handler
               (list (lambda (e) 43) (lambda () (raise-continuable 'x))))
        "#,
        On::TreeWalker,
        "43",
        CONTROL_OPS,
    );
}

// ─── Multi-value continuations (TREE_WALKER_CALLCC_MULTI_VALUES.md) ──────────

/// A `call/cc` continuation invoked with multiple values.
#[test]
fn callcc_multi_value_through_call_with_values() {
    assert_divergence(
        r#"
        (call-with-values
          (lambda ()
            (call-with-current-continuation
              (lambda (k) (k 1 2))))
          (lambda (a b) (list a b)))
        "#,
        On::Vm,
        "(1 2)",
        CALLCC_MULTI_VALUES,
    );
}

/// The abort pattern used by SRFI 1's `%cars+cdrs`.
#[test]
fn callcc_abort_pattern_through_call_with_values() {
    assert_divergence(
        r#"
        (call-with-values
          (lambda ()
            (call-with-current-continuation
              (lambda (abort)
                (abort '() '()))))
          (lambda (cars cdrs) (list 'cars cars 'cdrs cdrs)))
        "#,
        On::Vm,
        "(cars () cdrs ())",
        CALLCC_MULTI_VALUES,
    );
}

// ─── Continuation re-entry loses the handler stack (audit B3) ────────────────

/// Re-entering a continuation captured under `with-exception-handler` keeps
/// the handler on the VM (restored from its snapshot) but loses it on the
/// tree-walker: the escape path in `cps_eval/mod.rs` resets
/// `exception_handlers` to empty because `CpsContinuation` does not carry
/// them. Tree-walker: `unhandled continuable exception: boom`. The fix is to
/// store the handler stack (and `prompt_stack`) on `CpsContinuation` and
/// restore both on re-entry, as the VM does.
#[test]
fn reentered_continuation_keeps_exception_handler() {
    assert_divergence(
        r#"
        (define saved #f)
        (define entered #f)
        (define (run)
          (with-exception-handler
            (lambda (e) 42)
            (lambda ()
              (call/cc (lambda (k) (set! saved k) #f))
              (raise-continuable 'boom))))
        (let ((first (run)))
          (if entered
              (list 'second-pass first)
              (begin (set! entered #t) (saved #f))))
        "#,
        On::Vm,
        "(second-pass 42)",
        HANDLER_REENTRY,
    );
}

// ─── Not divergences ─────────────────────────────────────────────────────────

/// Both backends fail identically, so the differential harness cannot see this
/// — but it is still an R7RS conformance gap: it must evaluate to `1`.
/// Recorded so Q2 does not mistake backend *agreement* for correctness.
#[test]
fn apply_callcc_fails_on_both() {
    assert_program_eval_error("(apply call/cc (list (lambda (k) 1)))");
}

/// §1.2 recorded this as a VM failure (`Wrong number of arguments: expected 1,
/// got 2`) at `7a6a797`; both backends return `7` as of `2d4ce29`. Kept as the
/// regression guard for a row that was fixed without anyone noticing.
#[test]
fn apply_values_agrees() {
    assert_program_eval_to("(apply values (list 7))", "7");
}
