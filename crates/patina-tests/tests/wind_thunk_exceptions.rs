//! What happens when a `dynamic-wind` thunk raises during a continuation jump.
//!
//! The rule is R7RS 6.10: "The before and after thunks are called in the same
//! dynamic environment as the call to dynamic-wind" — and 6.11 puts the
//! exception-handler stack in that environment. So a thunk that a `guard`'s
//! escape runs still sees the guard's handler, the handler fires a second
//! time, and its second jump abandons the first: the after-thunk's exception
//! **replaces** the one in flight, unwinding **continues** through the outer
//! winds, and the replacement reaches the nearest handler enclosing the
//! `dynamic-wind`. That is Java's `finally` rule, and Gauche implements it
//! consistently across every shape below. chibi cannot arbitrate — it
//! overflows the stack on these probes — so Gauche is the oracle.
//!
//! **Both backends meet the rule on every shape here**, by the same two
//! mechanisms: a wind record captures the handler stack of its
//! `dynamic-wind` call, and a jump runs each thunk as a step of the machine
//! it was made on — a trampoline step on the tree-walker, a frame under a
//! one-instruction stub (`ResumeWindJump`) on the VM — under that stack.
//! Two further shapes that only the VM ever got wrong — a handler that is
//! *extra* at the jump, and a continuation captured inside a running
//! after-thunk — are in `backend_divergence.rs`.
//!
//! What neither backend covers is a raise from inside a Rust primitive's
//! callback within the thunk — `member`/`assoc` with a predicate,
//! `call-with-port`, `force`, a parameter converter, anything under `eval` —
//! which still runs on a nested trampoline (tree-walker) or a nested dispatch
//! loop (VM) with no handlers of its own (PRD §6, "a primitive's callback
//! runs on a nested trampoline with no handler stack"). No shape below
//! crosses one.
//!
//! Every test here is a plain `assert_program_eval_to`: one expectation, held
//! against both backends, and Gauche's answer in each case. Tracked in
//! `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6, "An exception raised by a
//! `dynamic-wind` after-thunk does not behave like a `finally`".

mod common;
use common::assert_program_eval_to;

/// Case 1 — a `guard` catches, and the after-thunk raises during its escape.
///
/// Gauche: `(one secondary)`. The clause runs **once** and sees the
/// *secondary*; the primary is discarded, exactly as a Java `finally` discards
/// the exception it replaces. The guard's handler fires twice — once for the
/// primary, again for the secondary raised by its own escape — because the
/// after-thunk runs under the handler stack of the `dynamic-wind` call, where
/// that handler is still installed.
///
/// The VM used to run the thunk with the handler already popped, so the
/// secondary cascaded uncaught and the program stopped, naming `secondary`.
/// (And before triage families 22 and 28 it answered `(one primary)`: the
/// after-thunk's exception silently discarded, which is the one answer the
/// rule most clearly forbids.)
#[test]
fn case1_after_thunk_raises_during_a_guard_escape() {
    assert_program_eval_to(
        r#"
        (guard (e (#t (list 'one e)))
          (dynamic-wind (lambda () #f)
                        (lambda () (raise 'primary))
                        (lambda () (raise 'secondary))))
    "#,
        "(one secondary)",
    );
}

/// Case 2 — nested guards; the inner one catches the primary.
///
/// Gauche: `(inner secondary)` — the handler nearest the `dynamic-wind`. The
/// VM used to give the *outer* guard, because the inner one's handler was
/// gone by the time the after-thunk ran.
#[test]
fn case2_nested_guards_see_the_after_thunk_exception() {
    assert_program_eval_to(
        r#"
        (guard (o (#t (list 'outer o)))
          (guard (i (#t (list 'inner i)))
            (dynamic-wind (lambda () #f)
                          (lambda () (raise 'primary))
                          (lambda () (raise 'secondary)))))
    "#,
        "(inner secondary)",
    );
}

/// Case 3 — the body exits normally and the after-thunk raises.
///
/// Both backends always matched Gauche: on a normal exit the live handler
/// stack *is* the `dynamic-wind` call's. Kept so neither fix could regress
/// the one shape that never needed one.
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
/// Both match Gauche, and both always did. A plain escape pops no handler, so
/// the VM's live stack happened to equal the call's; the tree-walker used to
/// run the thunk on a nested trampoline with no handlers at all.
#[test]
fn case4_after_thunk_raises_during_a_callcc_escape() {
    assert_program_eval_to(
        r#"
        (guard (e (#t (list 'escape e)))
          (call-with-current-continuation
            (lambda (k)
              (dynamic-wind (lambda () #f)
                            (lambda () (k 'escaped))
                            (lambda () (raise 'secondary))))))
    "#,
        "(escape secondary)",
    );
}

/// Nested winds, where the *inner* after-thunk raises: unwinding continues
/// outward past it.
///
/// Gauche: `((caught sec2) (in1 in2 out2 out1))`. The guard's second jump
/// starts from the wind stack the first jump had got to — the inner record
/// already popped — so it runs `out1` and then delivers `sec2`.
///
/// The second form wraps the same program in an outer guard, and is what
/// separated the two halves of the old VM answer: the VM always finished the
/// unwind when something outside caught (`(in1 in2 out2 out1)` either way),
/// but delivered to the outer guard, one handler too far out — case 2's gap.
/// With a single guard it stopped at `sec2` and never ran `out1`, because
/// nothing caught and an unhandled exception stops the program where it is
/// raised. Both forms are kept: the pair is what says the unwind was never
/// the defect, only who catches.
#[test]
fn nested_winds_finish_unwinding_before_delivering_the_replacement() {
    assert_program_eval_to(
        r#"
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
    "#,
        "((caught sec2) (in1 in2 out2 out1))",
    );

    assert_program_eval_to(
        r#"
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
    "#,
        "((caught sec2) (in1 in2 out2 out1))",
    );
}

/// A handler *inside* the body re-raises, then the after-thunk raises.
///
/// Gauche: `(outer secondary)`. Two handlers fire for the primary (the inner
/// one re-raises to the guard); the after-thunk still runs under the stack
/// the `dynamic-wind` call had, which holds the guard. On the VM both
/// handlers had been popped by then, and the secondary cascaded uncaught.
#[test]
fn inner_handler_reraises_then_after_thunk_raises() {
    assert_program_eval_to(
        r#"
        (guard (e (#t (list 'outer e)))
          (dynamic-wind (lambda () #f)
            (lambda ()
              (with-exception-handler
                (lambda (c) (raise (list 'from-inner c)))
                (lambda () (raise 'primary))))
            (lambda () (raise 'secondary))))
    "#,
        "(outer secondary)",
    );
}

/// `raise-continuable` from an after-thunk, with a handler that returns: the
/// thunk resumes, completes, and the original escape lands.
///
/// Gauche: `(x (sec))`. Both match, and both did before: a plain escape pops
/// nothing, so even the old "handlers from the machine" rule found the right
/// stack. Both now run the thunk as a step with the call's handlers, so the
/// handler's return goes back into the thunk.
#[test]
fn raise_continuable_in_after_thunk_resumes_the_thunk() {
    assert_program_eval_to(
        r#"
        (define log '())
        (define r
          (with-exception-handler
            (lambda (c) (set! log (cons c log)) 'ignored)
            (lambda ()
              (call/cc (lambda (k)
                (dynamic-wind (lambda () #f)
                              (lambda () (k 'x))
                              (lambda () (raise-continuable 'sec))))))))
        (list r (reverse log))
    "#,
        "(x (sec))",
    );
}

/// A `guard` declines, its `handler-k` re-enters the wind, and the
/// *before*-thunk raises on that second entry.
///
/// Gauche: `(outer before-raised ((before 1) after (before 2)))`. The before
/// thunk runs under the `dynamic-wind` call's stack — both guards — so the
/// inner guard gets a second look, declines again, and the outer one catches.
/// Its record is not on the wind stack while it runs, so the escape does not
/// run the after-thunk a second time: `after` appears once. The VM gives the
/// same answer, and used to reach it by a shorter route: the inner handler
/// was already gone, so the outer one fired directly.
#[test]
fn before_thunk_raises_on_reentry_after_a_guard_declines() {
    assert_program_eval_to(
        r#"
        (define n 0)
        (define log '())
        (guard (o (#t (list 'outer o (reverse log))))
          (guard (e ((eq? e 'never) 'never))
            (dynamic-wind
              (lambda ()
                (set! n (+ n 1))
                (set! log (cons (list 'before n) log))
                (if (= n 2) (raise 'before-raised)))
              (lambda () (raise 'primary))
              (lambda () (set! log (cons 'after log))))))
    "#,
        "(outer before-raised ((before 1) after (before 2)))",
    );
}

/// The after-thunk raises and the guard's clause *declines* the secondary:
/// the re-raise reaches the outer guard.
///
/// Gauche: `(outer secondary)`. Both match — and the VM used to match here by
/// accident, case 2's gap dropping the very handler this row's clause
/// declines to.
#[test]
fn guard_clause_declines_the_secondary() {
    assert_program_eval_to(
        r#"
        (guard (o (#t (list 'outer o)))
          (guard (e ((eq? e 'primary) (list 'inner e)))
            (dynamic-wind (lambda () #f)
                          (lambda () (raise 'primary))
                          (lambda () (raise 'secondary)))))
    "#,
        "(outer secondary)",
    );
}

/// No `guard` at all: a `with-exception-handler` handler escapes through a
/// continuation, and the after-thunk raises on the way out. The handler runs
/// again, for the secondary, and its second escape replaces the first.
///
/// Gauche: `(escaped secondary (primary secondary))`. On the VM the handler
/// had been popped by the first raise and the secondary cascaded uncaught.
#[test]
fn handler_escape_runs_the_handler_again_for_the_secondary() {
    assert_program_eval_to(
        r#"
        (define log '())
        (call/cc (lambda (k)
          (with-exception-handler
            (lambda (c) (set! log (cons c log)) (k (list 'escaped c (reverse log))))
            (lambda ()
              (dynamic-wind (lambda () #f)
                            (lambda () (raise 'primary))
                            (lambda () (raise 'secondary)))))))
    "#,
        "(escaped secondary (primary secondary))",
    );
}

/// The shape that decided both designs. The inner guard declines the
/// secondary; the outer handler *returns* from the continuable re-raise; so
/// the after-thunk must resume where `handler-k` re-entered it, complete, and
/// let the original escape land.
///
/// Gauche: `(escaped (secondary))`. Running the thunk as a step of the
/// machine the jump was made on makes that re-entry ordinary, because the
/// thing that owns "the rest of the jump" is a resumable frame — a `Jump`
/// continuation on the tree-walker, a `ResumeWindJump` stub frame on the VM.
/// Running it on a nested Rust call cannot: the nested frame is gone by the
/// time `handler-k` re-enters. The tree-walker's nested trampoline ended the
/// program with `'ignored` at the thunk's `Halt`; the VM re-entered the
/// enclosing frame *inside* the inlined `dynamic-wind` sequence, which ran
/// the after-thunk a second time and lost the escape's value —
/// `(() (secondary secondary))`.
#[test]
fn after_thunk_resumes_after_a_declining_guard_and_a_returning_handler() {
    assert_program_eval_to(
        r#"
        (define log '())
        (define r
          (with-exception-handler
            (lambda (c) (set! log (cons c log)) 'ignored)
            (lambda ()
              (guard (e ((eq? e 'never) 'never))
                (call/cc (lambda (k)
                  (dynamic-wind (lambda () #f)
                                (lambda () (k 'escaped))
                                (lambda () (raise-continuable 'secondary)))))))))
        (list r (reverse log))
    "#,
        "(escaped (secondary))",
    );
}
