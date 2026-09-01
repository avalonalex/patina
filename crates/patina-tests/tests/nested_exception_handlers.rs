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
use common::assert_program_eval_to;
use common::eval_program as eval;

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
        (reverse out)
        "#,
        "((else a) 42 (2 3) fell-through fell-through 5 str (outer inner) 3 \
         (1 2 3) () ok 42 inner (top obj))",
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
