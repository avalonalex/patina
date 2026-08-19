//! Parameter objects: `make-parameter`, `parameterize`, and what follows from
//! R7RS §4.2.6 making a parameter a procedure.
//!
//! These run on both backends deliberately: `procedure?` answered `#f` for
//! parameter objects on *both*, so a single-backend file could not have stated
//! the property it was missing. The predicate's own truth table lives with the
//! other predicates in `compliance/predicates.rs`; this file covers what being
//! a procedure lets a parameter *do*.

mod common;
use common::{assert_program_eval_error, assert_program_eval_to};

// ─── Reading, setting, parameterize ──────────────────────────────────────────

#[test]
fn test_make_parameter_basic() {
    assert_program_eval_to("(define p (make-parameter 10)) (p)", "10");
}

#[test]
fn test_parameter_set() {
    assert_program_eval_to("(define p (make-parameter 10)) (p 20) (p)", "20");
}

#[test]
fn test_parameterize_simple() {
    assert_program_eval_to(
        "(define p (make-parameter 10)) (parameterize ((p 20)) (p))",
        "20",
    );
}

#[test]
fn test_parameterize_restores() {
    assert_program_eval_to(
        "(define p (make-parameter 10)) (parameterize ((p 20)) (p)) (p)",
        "10",
    );
}

#[test]
fn test_parameterize_multiple_params() {
    assert_program_eval_to(
        r#"(define p1 (make-parameter 10))
           (define p2 (make-parameter 20))
           (parameterize ((p1 100) (p2 200)) (list (p1) (p2)))"#,
        "(100 200)",
    );
}

#[test]
fn test_parameterize_nested() {
    assert_program_eval_to(
        r#"(define p (make-parameter 10))
           (parameterize ((p 20)) (parameterize ((p 30)) (p)))"#,
        "30",
    );
}

#[test]
fn test_parameterize_nested_restores() {
    assert_program_eval_to(
        r#"(define p (make-parameter 10))
           (parameterize ((p 20)) (parameterize ((p 30)) (p)) (p))"#,
        "20",
    );
}

#[test]
fn test_parameter_with_converter() {
    // The converter is applied to the initial value per R7RS §4.2.6.
    assert_program_eval_to(
        "(define p (make-parameter 10 (lambda (x) (* x 2)))) (p)",
        "20",
    );
}

#[test]
fn test_parameter_converter_on_set() {
    assert_program_eval_to(
        "(define p (make-parameter 10 (lambda (x) (* x 2)))) (p 5) (p)",
        "10",
    );
}

#[test]
fn test_parameterize_empty_body_error() {
    assert_program_eval_error("(define p (make-parameter 10)) (parameterize ((p 20)))");
}

#[test]
fn test_parameterize_non_parameter_error() {
    assert_program_eval_error(r#"(parameterize ((42 20)) (display "hello"))"#);
}

#[test]
fn test_parameterize_body_sequence() {
    assert_program_eval_to(
        "(define p (make-parameter 10)) (parameterize ((p 20)) (p) (p) (p))",
        "20",
    );
}

// ─── A parameter is a procedure (R7RS §4.2.6) ────────────────────────────────

/// The predicate and the call path must agree: anything `procedure?` accepts
/// should be callable through the ordinary procedure routes. A parameter has
/// its own calling convention internally, which is exactly why this is worth
/// asserting rather than assuming. (Both routes worked before `procedure?` was
/// fixed — only the predicate lied — so this documents the contrast rather
/// than guarding the fix.)
#[test]
fn test_a_parameter_is_callable_the_ways_a_procedure_is() {
    assert_program_eval_to(
        r#"(define p (make-parameter 7))
           (list (apply p '()) (map (lambda (f) (f)) (list p)))"#,
        "(7 (7))",
    );
}

/// A parameter is accepted where a procedure is *required* — meaning the two
/// places that actually gate on the predicate. Both were verified to reject a
/// parameter before the fix, which is what makes them worth asserting.
///
/// `dynamic-wind` is deliberately absent: it type-checks nothing on either
/// backend and simply calls its thunks, so it accepted a parameter all along
/// and would pin nothing here.
#[test]
fn test_a_parameter_is_accepted_where_a_procedure_is_required() {
    // Rejected on both backends before the fix.
    assert_program_eval_to(
        "(define p (make-parameter 1)) (with-exception-handler p (lambda () 'ran))",
        "ran",
    );
    // `make-parameter`'s converter check is the line this change edited: it
    // used to read `is_procedure(c) || is_parameter(c)` and now relies on
    // `is_procedure` alone, so this is what would catch that removal going
    // wrong. `(inner 5)` sets `inner`, so reading it back proves the converter
    // was accepted *and* applied.
    assert_program_eval_to(
        r#"(define inner (make-parameter 0))
           (define p (make-parameter 5 inner))
           (inner)"#,
        "5",
    );
}

/// It prints as what it is. Before this it fell through the writer's chain to
/// `#<unknown>` — harmless until `procedure?` started saying `#t`, at which
/// point a value claiming to be a procedure printed as nothing in particular.
#[test]
fn test_a_parameter_prints_as_a_parameter() {
    assert_program_eval_to("(make-parameter 1)", "#<parameter>");
}

// ─── Convert once, before the wind (R7RS §4.2.6) ─────────────────────────────

/// D2 — the restore must not run the converter again.
///
/// `parameterize` restored by calling the parameter, and calling a parameter
/// converts. So the old value, which the converter had already produced, went
/// through it a second time on the way out.
#[test]
fn test_the_converter_does_not_run_again_on_restore() {
    assert_program_eval_to(
        "(define p (make-parameter 10 (lambda (x) (* x 2))))
         (define before (p))
         (define inside (parameterize ((p 1)) (p)))
         (list before inside (p))",
        "(20 2 20)",
    );
    // A type-changing converter makes the second application *raise*, so the
    // restore itself failed rather than merely producing the wrong value.
    assert_program_eval_to(
        "(define q (make-parameter 1 number->string))
         (define inside (parameterize ((q 2)) (q)))
         (list inside (q))",
        r#"("2" "1")"#,
    );
}

/// D1 — a `parameterize` that fails partway leaves nothing bound.
///
/// The bindings were installed inside `dynamic-wind`'s *before*-thunk, so a
/// later one raising meant the after-thunk never ran and the earlier ones
/// stayed installed for good.
#[test]
fn test_a_failed_parameterize_leaves_no_binding_changed() {
    // A converter that raises — the failure a real parameter can have.
    assert_program_eval_to(
        "(define a (make-parameter 'a0))
         (define b (make-parameter 'b0 (lambda (v) (if (eq? v 'bad) (error \"no\") v))))
         (define caught (guard (e (#t 'caught)) (parameterize ((a 'a1) (b 'bad)) 'body)))
         (list caught (a) (b))",
        "(caught a0 b0)",
    );
    // An install that raises — the failure the standard ports have, since they
    // are procedures over a thread-local and validate on assignment. #70 put
    // every program write behind this: a leak here sent all later output to
    // `sink` for the rest of the run.
    assert_program_eval_to(
        "(import (scheme base))
         (define sink (open-output-string))
         (define caught
           (guard (e (#t 'caught))
             (parameterize ((current-output-port sink) (current-input-port 5)) 'body)))
         (display \"visible\")
         (list caught (string-length (get-output-string sink)))",
        "(caught 0)",
    );
}

/// The converter runs once per `parameterize`, not once per entry — which is
/// what converting outside the wind buys beyond the two bugs above. A
/// continuation that re-enters the body re-installs the value; it must not
/// re-convert it.
#[test]
fn test_the_converter_runs_once_per_parameterize() {
    assert_program_eval_to(
        "(define calls 0)
         (define p (make-parameter 0 (lambda (v) (set! calls (+ calls 1)) v)))
         (define k #f)
         (define n 0)
         (parameterize ((p 1))
           (call/cc (lambda (c) (set! k c)))
           (set! n (+ n 1))
           (if (< n 2) (k #f)))
         (list n calls)",
        // One conversion for `(make-parameter 0 …)`, one for `(p 1)`.
        "(2 2)",
    );
}
