//! Continuations escaping past the VM's *internal* synchronous boundaries.
//!
//! Sibling of `escape_from_primitive.rs`, which covers the four
//! `ApplyContext` boundaries. Those were made correct in 2026-08-15 and stayed
//! correct; the boundaries here — the `run_thunk` calls the VM makes to itself
//! — were not, and #87 (control primitives callable as values) put user code
//! on the far side of them. Audit 2026-08-17, group A and C3.
//!
//! Every program is run on both backends by `assert_program_eval_to`. The
//! tree-walker answered all of them correctly before the fix, so it is the
//! expectation; chibi agrees with it on every program below except the one
//! marked, where chibi loops forever.
//!
//! ## The sweep (audit item A4)
//!
//! Every place the VM re-enters its own dispatch loop, and what guards it:
//!
//! | Site (`runtime/vm_state.rs`)                | Guard |
//! |---------------------------------------------|-------|
//! | `dynamic-wind` before / after thunk (value form) | `run_thunk` — propagates the sentinel |
//! | `dynamic-wind` body (value form)            | `run_thunk_outcome` — skips cleanup and `set_reg` |
//! | `call-with-values` producer (value form)    | `run_thunk_outcome` — skips the consumer |
//! | `vm_raise_value` exit winds                 | `run_thunk_outcome` — pops before running |
//! | `vm_raise_value` continuable handler        | `run_loop_until_outcome` — no re-push, no `set_reg` |
//! | `run_wind_transition` exit / enter thunks   | `run_thunk` — pops before running |
//! | `pop_resolved_winds`                        | `run_thunk` — already popped before running |
//! | `AbortCurrentContinuation` exit winds (both the control primitive and `Instruction::Abort`) | `run_thunk` — pops before running |
//! | `try_invoke_continuation` delimited enter thunks | `run_thunk` |
//! | `Instruction::InvokeContinuation` enter thunks | `run_thunk` |
//!
//! The decision itself lives in exactly one place: `run_loop_until_outcome`
//! compares the restored frame depth against its own `exit_depth`. Boundaries
//! do not each re-derive it — the two that did (`call_value_with_probe` and
//! its tail twin) got the `==` case wrong, which is how an escape into the
//! caller's own frame both clobbered a register and abandoned the rest of the
//! form.

mod common;
use common::assert_program_eval_to;

/// A2 — `dynamic-wind` called as a value, escaped out of from its body.
///
/// The register file is wide enough here that the write through the dead
/// frame lands out of bounds: before the fix this was a process abort in
/// `set_reg_at`, not a wrong answer.
#[test]
fn test_escape_from_a_dynamic_wind_body_called_as_a_value() {
    assert_program_eval_to(
        "(define dw dynamic-wind)
         (define (bad k)
           (let ((x1 1) (x2 2) (x3 3) (x4 4) (x5 5) (x6 6) (x7 7) (x8 8)
                 (x9 9) (x10 10) (x11 11) (x12 12) (x13 13) (x14 14) (x15 15))
             (let ((r (dw (lambda () 0) (lambda () (k 7) 99) (lambda () 0))))
               (+ r x1 x2 x3 x4 x5 x6 x7 x8 x9 x10 x11 x12 x13 x14 x15))))
         (call/cc (lambda (k) (bad k) 999))",
        "7",
    );
}

/// The other half of A2: the escape must also *resume* the form it returns
/// into. Reporting the escape as a normal return unwound every enclosing
/// dispatch loop, including the one that still owned the restored frame, so
/// the rest of the `let` never ran.
#[test]
fn test_the_form_the_escape_returns_into_still_runs() {
    assert_program_eval_to(
        "(define dw dynamic-wind)
         (let ((r (call/cc (lambda (k)
                    (dw (lambda () 0) (lambda () (k 7) 99) (lambda () 0))
                    999))))
           (list 'resumed r))",
        "(resumed 7)",
    );
}

#[test]
fn test_escape_from_the_before_and_after_thunks_called_as_a_value() {
    assert_program_eval_to(
        "(define dw dynamic-wind)
         (call/cc (lambda (k)
           (dw (lambda () (k 'from-before)) (lambda () 'body) (lambda () 'after))))",
        "from-before",
    );
    assert_program_eval_to(
        "(define dw dynamic-wind)
         (call/cc (lambda (k)
           (dw (lambda () 0) (lambda () 'body) (lambda () (k 'from-after)))))",
        "from-after",
    );
}

/// An escape out of the value form still runs the after-thunk exactly once —
/// the wind transition the continuation performs is what runs it, which is
/// why the abandoned call must *not* run its own cleanup as well.
#[test]
fn test_the_after_thunk_runs_once_on_escape() {
    assert_program_eval_to(
        "(define dw dynamic-wind)
         (define log '())
         (define r (call/cc (lambda (k)
           (dw (lambda () (set! log (cons 'in log)))
               (lambda () (k 'escaped))
               (lambda () (set! log (cons 'out log)))))))
         (list r (reverse log))",
        "(escaped (in out))",
    );
}

/// C3 — `call-with-values` as a value, producer escapes. The consumer ran
/// anyway, on the escape value, and its result then replaced it.
#[test]
fn test_escape_from_a_call_with_values_producer_called_as_a_value() {
    assert_program_eval_to(
        "(define cwv call-with-values)
         (define ran 'no)
         (define r (call/cc (lambda (k)
           (cwv (lambda () (k 42)) (lambda vs (set! ran 'consumer-ran) 99)))))
         (list r ran)",
        "(42 no)",
    );
}

/// A3 — a continuation invoked from an after-thunk while `raise` unwinds.
///
/// The record was still on `dynamic_winds` while its own after-thunk ran, so
/// the escape re-entered it and re-ran the same thunk, with no depth guard:
/// `run_thunk → try_invoke_continuation → run_wind_transition → run_thunk`
/// until the native stack gave out and aborted the process.
///
/// The one program here chibi cannot arbitrate — it loops on this too, and
/// dies with a clean "out of stack space". Both Patina backends now answer;
/// the tree-walker always did.
#[test]
fn test_escape_from_an_after_thunk_during_raise_unwinding() {
    assert_program_eval_to(
        "(call/cc (lambda (k)
           (guard (e (#t (list 'caught e)))
             (dynamic-wind (lambda () 0)
                           (lambda () (raise 'boom))
                           (lambda () (k 'escaped-from-after))))))",
        "escaped-from-after",
    );
}

/// A4 — the continuable-`raise` handler path, which runs its handler
/// synchronously and then re-pushes the handler and writes a register.
#[test]
fn test_escape_from_a_raise_continuable_handler() {
    assert_program_eval_to(
        "(call/cc (lambda (k)
           (with-exception-handler (lambda (e) (k (list 'handler-escaped e)))
             (lambda () (+ 1 (raise-continuable 'oops))))))",
        "(handler-escaped oops)",
    );
}

// ─── Not escapes ─────────────────────────────────────────────────────────────
//
// The fix turns a continuation invocation into an unwind, so these say that
// the ordinary uses still resume rather than exit.

#[test]
fn test_the_value_forms_still_work_without_any_escape() {
    assert_program_eval_to(
        "(define dw dynamic-wind)
         (define log '())
         (define r (dw (lambda () (set! log (cons 'in log)))
                       (lambda () 'body)
                       (lambda () (set! log (cons 'out log)))))
         (list r (reverse log))",
        "(body (in out))",
    );
    assert_program_eval_to(
        "(define cwv call-with-values) (cwv (lambda () (values 1 2)) list)",
        "(1 2)",
    );
}

/// A continuation captured *inside* a wind body and re-invoked there is an
/// in-extent jump, not an escape: the loop that owns the restored frame keeps
/// dispatching.
#[test]
fn test_a_continuation_used_inside_a_wind_body_is_not_an_escape() {
    assert_program_eval_to(
        "(define n 0)
         (dynamic-wind
           (lambda () 0)
           (lambda () (let ((r (call/cc (lambda (c) c))))
                        (set! n (+ n 1))
                        (if (procedure? r) (r n) (list 'retried n))))
           (lambda () 0))",
        "(retried 2)",
    );
}
