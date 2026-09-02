//! What happens when a `dynamic-wind` *after* thunk raises.
//!
//! R7RS says nothing about this, so the oracle is Gauche, which implements
//! Java's `finally` rule and does so consistently across every shape probed:
//! the after-thunk's exception **replaces** the one in flight, unwinding
//! **continues** through the outer winds, and the replacement is delivered to
//! the nearest handler enclosing the `dynamic-wind`. chibi cannot arbitrate —
//! it overflows the stack on all four probes below.
//!
//! We meet that rule in 2 of 4 cases on the VM and 1 of 4 on the tree-walker.
//! These tests assert **today's answers**, not Gauche's, so that the fix trips
//! them and has to update the record deliberately. Each one names what Gauche
//! says, so the target is written down rather than remembered.
//!
//! Tracked in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6, "An exception raised by
//! a `dynamic-wind` after-thunk does not behave like a `finally`".

mod common;
use common::{
    assert_program_eval_to, eval_program_vm, try_eval_program_tree_walker, try_eval_program_vm,
};

/// The recorded answer for a shape neither backend catches: the program stops,
/// and the message names the *secondary* exception. `who` names the backend
/// in the panic message.
fn expect_stops_naming_secondary(result: Result<String, String>, who: &str) {
    match result {
        Err(message) => assert!(
            message.contains("secondary") || message.contains("sec2"),
            "\n[{who}] stops, but not on the secondary exception: {message}"
        ),
        Ok(value) => panic!(
            "\n[{who}] NO LONGER STOPS — it answered {value:?}.\nIf this is Gauche's \
             answer, the finally rule landed: assert it directly and update PRD §6."
        ),
    }
}

/// Case 1 — a `guard` catches, and the after-thunk raises during its escape.
///
/// Gauche: `(one secondary)`. Its clause runs **once** and sees the
/// *secondary*; the primary is discarded, exactly as a Java `finally` discards
/// the exception it replaces. That needs the guard's handler to fire twice —
/// once for the primary, again for the secondary raised by its own escape —
/// which neither backend does.
///
/// This is the only one of the four that moved with triage families 22 and 28.
/// Before them the VM answered `(one primary)`: the after-thunk's exception was
/// silently discarded, which is the one answer the rule most clearly forbids.
/// It now cascades and stops the program, which is closer to the rule but still
/// short of it.
#[test]
fn case1_after_thunk_raises_during_a_guard_escape() {
    const PROGRAM: &str = r#"
        (guard (e (#t (list 'one e)))
          (dynamic-wind (lambda () #f)
                        (lambda () (raise 'primary))
                        (lambda () (raise 'secondary))))
    "#;
    // Neither backend catches it; both stop the program. Gauche: (one secondary).
    expect_stops_naming_secondary(try_eval_program_vm(PROGRAM), "vm");
    expect_stops_naming_secondary(try_eval_program_tree_walker(PROGRAM), "tree-walker");
}

/// Case 2 — nested guards; the inner one catches the primary.
///
/// Gauche: `(inner secondary)` — the handler nearest the `dynamic-wind`. The VM
/// gives the *outer* guard, because the inner one's handler is gone by the time
/// the after-thunk runs. The tree-walker escapes entirely.
///
/// Unchanged by families 22/28 on both backends.
#[test]
fn case2_nested_guards_see_the_after_thunk_exception() {
    const PROGRAM: &str = r#"
        (guard (o (#t (list 'outer o)))
          (guard (i (#t (list 'inner i)))
            (dynamic-wind (lambda () #f)
                          (lambda () (raise 'primary))
                          (lambda () (raise 'secondary)))))
    "#;
    assert_eq!(
        eval_program_vm(PROGRAM),
        "(outer secondary)",
        "VM reaches the outer guard; Gauche reaches the inner one"
    );
    expect_stops_naming_secondary(try_eval_program_tree_walker(PROGRAM), "tree-walker");
}

/// Case 3 — the body exits normally and the after-thunk raises.
///
/// Both backends already match Gauche. Kept so the fix cannot regress the one
/// shape that works everywhere.
#[test]
fn case3_after_thunk_raises_on_a_normal_exit() {
    assert_program_eval_to(
        r#"
        (guard (e (#t (list 'normal-exit e)))
          (dynamic-wind (lambda () #f)
                        (lambda () 'body-ok)
                        (lambda () (raise 'secondary))))
    "#,
        "(normal-exit secondary)",
    );
}

/// Case 4 — an ordinary `call/cc` escape, with the after-thunk raising.
///
/// The VM matches Gauche; the tree-walker escapes, for the same
/// nested-trampoline reason as case 2.
#[test]
fn case4_after_thunk_raises_during_a_callcc_escape() {
    const PROGRAM: &str = r#"
        (guard (e (#t (list 'escape e)))
          (call-with-current-continuation
            (lambda (k)
              (dynamic-wind (lambda () #f)
                            (lambda () (k 'escaped))
                            (lambda () (raise 'secondary))))))
    "#;
    assert_eq!(eval_program_vm(PROGRAM), "(escape secondary)");
    expect_stops_naming_secondary(try_eval_program_tree_walker(PROGRAM), "tree-walker");
}

/// A nested wind, where the *inner* after-thunk raises: does unwinding
/// continue outward past it?
///
/// Gauche answers `((caught sec2) (in1 in2 out2 out1))` — the outer
/// after-thunk still runs, then the replacement is delivered. Ours stops the
/// program at `sec2`, and the outer thunk does **not** run: measured through
/// files the abort cannot swallow, both backends' logs read `in1 in2 out2`.
///
/// That is a *consequence* of case 1, not a second gap. The `guard`'s handler
/// is gone when `sec2` is raised, nothing outside catches it, and an
/// unhandled exception stops the program where it is raised — no after-thunk
/// runs after a fatal error on any implementation. Put a handler outside and
/// the VM continues the unwind and runs `out1` before delivering `sec2` — to
/// the outer `guard`, which is case 2's gap, not this one. The second program
/// pins that, so the case-1 fix can rely on the unwind machinery it needs: it
/// is already there. The tree-walker still escapes for the nested-trampoline
/// reason.
///
/// Before families 22 and 28 the VM answered `((caught primary) (in1 in2 out2
/// out1))` to the first program: it continued unwinding, then discarded `sec2`
/// entirely.
#[test]
fn an_uncaught_inner_after_thunk_exception_stops_before_the_outer_thunk() {
    const PROGRAM: &str = r#"
        (define log '())
        (define caught
          (guard (e (#t (list 'caught e)))
            (dynamic-wind (lambda () (set! log (cons 'in1 log)))
              (lambda ()
                (dynamic-wind (lambda () (set! log (cons 'in2 log)))
                  (lambda () (raise 'primary))
                  (lambda () (set! log (cons 'out2 log)) (raise 'sec2))))
              (lambda () (set! log (cons 'out1 log))))))
        (list caught (reverse log))
    "#;
    expect_stops_naming_secondary(try_eval_program_vm(PROGRAM), "vm");
    expect_stops_naming_secondary(try_eval_program_tree_walker(PROGRAM), "tree-walker");

    const WITH_OUTER_GUARD: &str = r#"
        (define log '())
        (define caught
          (guard (o (#t (list 'outer o)))
            (guard (e (#t (list 'caught e)))
              (dynamic-wind (lambda () (set! log (cons 'in1 log)))
                (lambda ()
                  (dynamic-wind (lambda () (set! log (cons 'in2 log)))
                    (lambda () (raise 'primary))
                    (lambda () (set! log (cons 'out2 log)) (raise 'sec2))))
                (lambda () (set! log (cons 'out1 log)))))))
        (list caught (reverse log))
    "#;
    assert_eq!(
        eval_program_vm(WITH_OUTER_GUARD),
        "((outer sec2) (in1 in2 out2 out1))",
        "the VM finishes the unwind when something outside catches; Gauche: \
         ((caught sec2) (in1 in2 out2 out1))"
    );
    expect_stops_naming_secondary(
        try_eval_program_tree_walker(WITH_OUTER_GUARD),
        "tree-walker",
    );
}
