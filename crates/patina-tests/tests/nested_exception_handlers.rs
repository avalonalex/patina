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
/// extent** (R7RS §4.2.7), so a `dynamic-wind` between the two guards should
/// be re-entered before the re-raise and exited again after it.
///
/// Patina does not rewind: it re-raises where the `guard` stands. Both
/// backends agree, so this is a deviation from the report and from chibi
/// rather than a divergence — recorded here because it is observable, and
/// only with side-effecting wind thunks (audit F9).
///
/// ```text
/// Patina  => (in out)          chibi => (in out in out)
/// ```
///
/// Fixing it means the re-raise carrying a continuation back into the raiser's
/// wind stack, which is `guard`'s expansion, not the handler machinery. The
/// value the whole expression produces is the same either way, which is why
/// nothing else notices.
#[test]
fn test_a_guard_re_raise_does_not_rewind_into_the_raiser() {
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
        "((outer boom) (in out))"
    );
}
