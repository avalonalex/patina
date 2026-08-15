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
