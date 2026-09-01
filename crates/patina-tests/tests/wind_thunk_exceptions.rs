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
/// and the message names the *secondary* exception. Returns the message so a
/// caller can say which backend it came from.
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
/// continue outward?
///
/// Gauche answers `((caught sec2) (in1 in2 out2 out1))` — the outer
/// after-thunk still runs, then the replacement is delivered. Ours stops the
/// program, and the outer thunk does **not** run: measured through a
/// file the abort cannot swallow, the VM's log reads `(in1 in2 out2)`.
///
/// So this is a second half of the same gap. It is case 1's shape (a `guard`
/// escape meeting a failing after-thunk), and it shows that "unwinding
/// continues past the thunk that raised" is *also* something we do not do —
/// worth knowing, because it means the fix is not only about who catches.
///
/// Before families 22 and 28 the VM answered `((caught primary) (in1 in2 out2
/// out1))`: it continued unwinding, then discarded `sec2` entirely.
#[test]
fn a_failing_inner_after_thunk_stops_the_outward_unwind() {
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
}
