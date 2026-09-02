//! Capturing a continuation from inside an effect-carrying continuation.
//!
//! `with-exception-handler` wraps its thunk's continuation in an
//! `ExceptionHandlerCleanup` so the handler is popped on the way out. The
//! tree-walker's continuation serializer had no representation for that
//! wrapper — or for the five others like it — and dropped the whole entry from
//! the captured continuation environment. The captured body still referred to
//! that binder, so re-entering the continuation failed with
//! `Undefined variable: k_N`, where `k_N` is a CPS-transform gensym.
//!
//! `guard` expands to `call/cc` + `with-exception-handler`, so any `guard`
//! nested inside another exception handler tripped it. That is 15 tests in the
//! R7RS suite, all in Control Features and Exceptions — invisible until Patina
//! adopted upstream `(chibi test)`, whose applier calls each test thunk from
//! inside its own `guard`.
//!
//! The VM was never affected: it snapshots whole machine state for `call/cc`
//! rather than serializing continuations name by name, so there is no per-frame
//! case to leave unimplemented.

mod common;
use common::eval_program as eval;
use common::{ErrorClass, assert_program_eval_error_at, assert_program_eval_to};

/// Every shape of `guard` R7RS §4.2.7 defines, in one program, checked against
/// chibi and Gauche — which produce this line character for character.
///
/// `guard` was rewritten on 2026-09-01 to R7RS 7.3's expansion, bar one
/// deliberate deviation on the success path (triage families 22 and 28; the
/// reason is in `lib/scheme/base/exceptions.scm`). The new expansion routes
/// the body's *normal* result through a `call-with-values` and a thunk where
/// the previous one returned it directly, so the shapes at risk are not the
/// exception ones: they are multiple values, zero values, and a body whose
/// definitions must stay in a body context. The clause walker moved from
/// `cond` to its own `%guard-aux`, which puts `else` and `=>` at risk too, and
/// `var` is bound by a `let` inside a template, which puts hygiene at risk.
#[test]
fn guard_covers_every_r7rs_clause_shape() {
    assert_program_eval_to(
        r#"
        (define out '())
        (define (show x) (set! out (cons x out)))
        (show (guard (e (else (list 'else e))) (raise 'a)))
        (show (guard (e ((assq 'b e) => cdr)) (raise (list (cons 'b 42)))))
        (show (guard (e ((memv e '(1 2 3)))) (raise 2)))
        ;; the two NON-terminal walker rules: `=>` and a bare test, each with
        ;; a clause after them, which the terminal rules would otherwise hide
        (show (guard (e ((assq 'b e) => cdr) (#t 'fell-through)) (raise (list (cons 'c 1)))))
        (show (guard (e ((memv e '(7 8)) ) (#t 'fell-through)) (raise 9)))
        (show (guard (e ((assq 'b e) => cdr) (#t 'no)) (raise (list (cons 'b 5)))))
        (show (guard (e ((symbol? e) 'sym) ((string? e) 'str)) (raise "s")))
        (show (guard (o (#t (list 'outer o)))
                (guard (e ((string? e) 'str)) (raise 'inner))))
        (show (guard (e (#t 'never)) 1 2 3))
        (show (call-with-values (lambda () (guard (e (#t 'never)) (values 1 2 3))) list))
        (show (call-with-values (lambda () (guard (e (#t 'never)) (values))) list))
        (show (let ((else #f)) (guard (e ((symbol? e) 'sym)) 'ok)))
        (show (guard (e (#t 'never)) (define x 7) (* x 6)))
        (show (let ((e 'outer)) (guard (e (#t e)) (raise 'inner))))
        (show (guard (a (#t (list 'top a)))
                (guard (b ((string? b) 'no))
                  (guard (c ((number? c) 'no))
                    (raise 'obj)))))
        ;; no clauses at all — R7RS 7.1.3 allows it (`<cond clause>*`), and
        ;; it means "re-raise", on both raise forms
        (show (guard (o (#t (list 'outer o))) (guard (e) (raise 'x))))
        (show (with-exception-handler (lambda (c) (list 'h c))
                (lambda () (guard (e) (list 'body (raise-continuable 'y))))))
        (reverse out)
        "#,
        "((else a) 42 (2 3) fell-through fell-through 5 str (outer inner) 3 \
         (1 2 3) () ok 42 inner (top obj) (outer x) (body (h y)))",
    );
}

/// The success path's deliberate deviation from R7RS 7.3, pinned from the
/// side that matters: a `guard` whose body returns normally inside a
/// primitive's callback must leave that callback running. The reference line
/// jumps to `guard-k` even on success; on the tree-walker every such jump
/// reads as an escape from the nested trampoline, and `call-with-port` closed
/// the port under a callback that then read from it. Returning in place is
/// what keeps the second `read-char` alive. Holds on the VM either way, so
/// this is also the assertion that must keep passing when the reference line
/// is restored (`lib/scheme/base/exceptions.scm` says when).
#[test]
fn a_guard_that_succeeds_inside_a_callback_leaves_the_callbacks_port_open() {
    assert_program_eval_to(
        r#"
        (call-with-port (open-input-string "abc")
          (lambda (p)
            (guard (e (#t 'no)) (read-char p))
            (read-char p)))
        "#,
        "#\\b",
    );
}

/// `with-exception-handler` pops its handler when its thunk returns — and a
/// thunk that ends in a tail call returns through a different VM path per
/// callee. The VM popped only on `Return`, which a closure callee reaches; a
/// control primitive, `values`, a primitive and a parameter each delivered
/// straight to the caller and left the handler installed for the rest of the
/// program. `guard`'s reference expansion ends its success path in `(apply
/// values args)`, so every successful `guard` under a
/// `with-exception-handler` leaked (found by review of triage families 22/28,
/// 2026-09-01), and the first shape below was the `(values 1)` leak already
/// present before them.
///
/// A leaked handler is visible only when nothing else is installed, so the
/// probe is an unrelated error afterwards: it must stop the program as
/// `car`'s own error, not reach `leak`'s handler and be re-raised as `leaked`.
#[test]
fn a_handler_thunk_that_ends_in_a_tail_call_still_pops_its_handler() {
    for thunk in [
        "(values 1)",                 // TailCallWithValues
        "(guard (e (#f 'no)) 'fine)", // `(apply values args)` at the end of a guard
        "(car '(1))",                 // a primitive
        "(p)",                        // a parameter
        "(call/cc (lambda (k) 1))",   // a control primitive
    ] {
        assert_program_eval_error_at(
            &format!(
                "(define p (make-parameter 1))
                 (define (leak thunk) (with-exception-handler (lambda (e) (raise 'leaked)) thunk))
                 (leak (lambda () {thunk}))
                 (car 5)"
            ),
            ErrorClass::AtRuntime,
            ErrorClass::AtRuntime,
            "car",
        );
    }
}

/// The same pop where the thunk returns to a *nested* run loop — a
/// `call-with-port` callback, or the body of `dynamic-wind` reached as a
/// value — rather than to a frame. That return does not go through the
/// frame-depth test at all: the loop closes the handlers installed under it
/// from its own entry count, because at its exit depth the frame-depth test
/// cannot tell a handler it was started under from one installed inside it
/// (the next test is the other side of that ambiguity).
#[test]
fn a_handler_installed_inside_a_nested_run_is_popped_when_the_run_returns() {
    for body in [
        "(call-with-port (open-input-string \"a\") (lambda (port) (leak (lambda () 'x))))",
        "(dw (lambda () #f) (lambda () (leak (lambda () 'x))) (lambda () #f))",
    ] {
        assert_program_eval_error_at(
            &format!(
                "(define dw dynamic-wind)
                 (define (leak thunk) (with-exception-handler (lambda (e) (raise 'leaked)) thunk))
                 {body}
                 (car 5)"
            ),
            ErrorClass::AtRuntime,
            ErrorClass::AtRuntime,
            "car",
        );
    }
}

/// The other side: a handler that a nested run loop was *started under* must
/// survive that run. The thunk tail-calls `dynamic-wind` as a value, so its
/// own frame is gone by the time `before` runs on a nested loop — the handler
/// sits at exactly that loop's exit depth, indistinguishable by depth from
/// one whose thunk has returned, and still owed `body`'s raise. The first
/// review fix for the leak above popped it at `before`'s return, and this
/// program lost its handler. chibi and Gauche: `(handled (in handler out))`.
#[test]
fn a_handler_survives_a_nested_run_that_its_thunks_tail_call_started() {
    assert_program_eval_to(
        r#"
        (define dw dynamic-wind)
        (define v '())
        (define (log x) (set! v (cons x v)))
        (define answer
          (with-exception-handler
            (lambda (e) (log 'handler) 'handled)
            (lambda ()
              (dw (lambda () (log 'in))
                  (lambda () (raise-continuable 'x))
                  (lambda () (log 'out))))))
        (list answer (reverse v))
        "#,
        "(handled (in handler out))",
    );
}

/// The handler and the `guard` clauses run in *different* dynamic
/// environments, and a parameter says which is which more directly than a
/// wind log does. chibi and Gauche produce this line exactly.
///
/// R7RS 6.11 runs the handler "in the dynamic environment of the call to
/// `raise`", so it reads `inner`. R7RS 4.2.7 runs the clauses "in the dynamic
/// environment of the `guard` expression", so they read `outer` — `guard-k`
/// has already left the `parameterize` by then. Before triage families 22 and
/// 28 the raise path unwound first and the handler read `outer` too, which
/// collapsed the distinction this test exists to hold.
#[test]
fn a_handler_and_a_guard_clause_run_in_different_dynamic_environments() {
    assert_program_eval_to(
        r#"
        (define p (make-parameter 'outer))
        (list
          (guard (e (#t (list 'clause (p)))) (parameterize ((p 'inner)) (raise 'x)))
          (with-exception-handler (lambda (e) (list 'handler (p)))
            (lambda () (parameterize ((p 'inner)) (raise-continuable 'x))))
          (parameterize ((p 'mid)) (guard (e (#t (list 'nested (p)))) (raise 'x))))
        "#,
        "((clause outer) (handler inner) (nested mid))",
    );
}

#[test]
fn test_nested_guard_through_a_thunk() {
    assert_eq!(
        eval(
            "(define (run th) (guard (e (#t (list 'outer e))) (list 'ok (th)))) \
             (run (lambda () (guard (x (else 'inner)) (error \"boom\"))))"
        ),
        "(ok inner)"
    );
}

#[test]
fn test_nested_guard_without_an_intervening_procedure() {
    assert_eq!(
        eval(
            "(guard (e (#t 'outer)) \
               (list 'ok (guard (x (else 'inner)) (error \"boom\"))))"
        ),
        "(ok inner)"
    );
}

/// The inner clause does not match, so the outer handler must still fire —
/// confirming the fix restores the continuation rather than swallowing the
/// raise.
#[test]
fn test_inner_guard_declines_and_outer_handles() {
    assert_eq!(
        eval(
            "(guard (e (#t (list 'outer e))) \
               (list 'ok (guard (x ((symbol? x) 'inner)) (raise 42))))"
        ),
        "(outer 42)"
    );
}

/// The underlying shape, with no `guard` macro involved: a non-tail `call/cc`
/// inside a `with-exception-handler` thunk. In tail position the wrapper is the
/// continuation itself and was already handled, which is why this needed the
/// value to be consumed by an enclosing form.
#[test]
fn test_non_tail_callcc_inside_exception_handler_thunk() {
    assert_eq!(
        eval(
            "(with-exception-handler (lambda (c) 'outer) \
               (lambda () (list 'ok (call-with-current-continuation (lambda (k) (k 'jump))))))"
        ),
        "(ok jump)"
    );
}

/// `CallWithValuesConsumer` is one of the other five wrappers that were being
/// dropped. Same defect, different variant.
#[test]
fn test_non_tail_callcc_inside_call_with_values_producer() {
    assert_eq!(
        eval(
            "(call-with-values \
               (lambda () (list 'ok (call-with-current-continuation (lambda (k) (k 'j))))) \
               (lambda (a) a))"
        ),
        "(ok j)"
    );
}

/// `dynamic-wind` has its own serialization and always worked; this pins that
/// the shared unwrap path did not disturb it.
#[test]
fn test_guard_under_dynamic_wind_still_works() {
    assert_eq!(
        eval(
            "(dynamic-wind (lambda () 1) \
                           (lambda () (guard (e (else 'caught)) (error \"x\"))) \
                           (lambda () 3))"
        ),
        "caught"
    );
}

#[test]
fn test_three_levels_of_nesting() {
    assert_eq!(
        eval(
            "(guard (a (#t (list 'l1 a))) \
               (list 'x (guard (b ((string? b) 'l2)) \
                 (list 'y (guard (c ((symbol? c) 'l3)) (raise 7))))))"
        ),
        "(l1 7)"
    );
}

/// A `guard` whose clauses all fail re-raises **in the raiser's dynamic
/// extent** (R7RS §4.2.7), so a `dynamic-wind` between the two guards is
/// re-entered before the re-raise and exited again after it. Fixed
/// 2026-09-01 with Track L triage families 22 and 28.
///
/// Patina used to re-raise where the `guard` stands, on both backends —
/// a deviation from chibi rather than a divergence, and observable only with
/// side-effecting wind thunks (audit F9), because the value the whole
/// expression produces is the same either way.
///
/// This pin outlived two wrong diagnoses, and both are worth keeping:
///
///  - "the fix is `guard`'s expansion, not the handler machinery" — wrong,
///    measured 2026-08-25. R7RS 7.3's reference `guard` carries exactly the
///    continuation back into the raiser's wind stack and *still* gave
///    `(in out)`, because the raise path unwound before calling the handler,
///    so that continuation was captured after the extent had been left.
///  - "so it is the handler machinery, not `guard`" — also wrong. Neither
///    half moves this alone. It took four changes together: the handler stack
///    on `CpsContinuation` (#150), the VM's wind common prefix (#149), no
///    unwind on any raise path, and the reference `guard`.
#[test]
fn test_a_guard_re_raise_rewinds_into_the_raiser() {
    assert_eq!(
        eval(
            "(import (scheme base))
             (define log '())
             (define result
               (guard (e ((symbol? e) (list 'outer e)))
                 (guard (e ((string? e) 'inner))
                   (dynamic-wind
                     (lambda () (set! log (cons 'in log)))
                     (lambda () (raise 'boom))
                     (lambda () (set! log (cons 'out log)))))))
             (list result (reverse log))"
        ),
        "((outer boom) (in out in out))"
    );
}
