//! Tests for R7RS (scheme r5rs) compatibility library

mod common;
use common::*;

// =============================================================================
// R5RS aliases
// =============================================================================

#[test]
fn test_exact_to_inexact() {
    assert_program_eval_to("(import (scheme r5rs)) (exact->inexact 3)", "3.0");
}

#[test]
fn test_inexact_to_exact() {
    assert_program_eval_to("(import (scheme r5rs)) (inexact->exact 3.0)", "3");
}

#[test]
fn test_exact_to_inexact_rational() {
    assert_program_eval_to(
        "(import (scheme r5rs)) (exact->inexact 1/3)",
        "0.3333333333333333",
    );
}

/// A library whose only import is (scheme r5rs) has no other way to reach
/// `define`, `lambda`, `if` or `quote` — syntax keywords are real bindings
/// and R5RS defines them, so the library must export them. srfi-78's
/// reference implementation in the vendored corpus is exactly this shape;
/// before the keywords were exported it failed with `unbound variable:
/// define`. Top level does not cover this: core syntax is seeded there.
#[test]
fn test_r5rs_provides_core_syntax_inside_a_library_body() {
    assert_program_eval_to(
        "(define-library (t r5rs-only)
           (export f)
           (import (scheme r5rs))
           (begin (define (f) (if #t 'ok 'no))))
         (import (t r5rs-only))
         (f)",
        "ok",
    );
}

// =============================================================================
// R5RS re-exports from various libraries
// =============================================================================

#[test]
fn test_r5rs_char_ops() {
    assert_program_eval_to("(import (scheme r5rs)) (char-alphabetic? #\\a)", "#t");
}

#[test]
fn test_r5rs_char_ci() {
    assert_program_eval_to("(import (scheme r5rs)) (char-ci=? #\\A #\\a)", "#t");
}

#[test]
fn test_r5rs_string_ci() {
    assert_program_eval_to(r#"(import (scheme r5rs)) (string-ci=? "ABC" "abc")"#, "#t");
}

#[test]
fn test_r5rs_complex() {
    assert_program_eval_to("(import (scheme r5rs)) (make-rectangular 3 4)", "3+4i");
}

#[test]
fn test_r5rs_inexact_sqrt() {
    assert_program_eval_to("(import (scheme r5rs)) (sqrt 4.0)", "2.0");
}

#[test]
fn test_r5rs_trig() {
    assert_program_eval_to("(import (scheme r5rs)) (sin 0.0)", "0.0");
}

#[test]
fn test_r5rs_delay_force() {
    assert_program_eval_to("(import (scheme r5rs)) (force (delay (+ 1 2)))", "3");
}

#[test]
fn test_r5rs_cxr() {
    assert_program_eval_to("(import (scheme r5rs)) (caaaar '((((42)))))", "42");
}

#[test]
fn test_r5rs_dynamic_wind() {
    assert_program_eval_to(
        "(import (scheme r5rs)) (dynamic-wind (lambda () #f) (lambda () 99) (lambda () #f))",
        "99",
    );
}

#[test]
fn test_r5rs_eval() {
    assert_program_eval_to(
        "(import (scheme r5rs)) (eval '(+ 1 2) (scheme-report-environment 5))",
        "3",
    );
}

#[test]
fn test_r5rs_numeric_predicates() {
    assert_program_eval_to(
        "(import (scheme r5rs)) (list (positive? 1) (negative? -1) (odd? 3) (even? 4) (zero? 0))",
        "(#t #t #t #t #t)",
    );
}

#[test]
fn test_r5rs_string_ops() {
    assert_program_eval_to(
        r#"(import (scheme r5rs)) (string-copy "hello")"#,
        r#""hello""#,
    );
}

#[test]
fn test_r5rs_vector_ops() {
    assert_program_eval_to(
        "(import (scheme r5rs)) (let ((v (make-vector 3 0))) (vector-fill! v 7) v)",
        "#(7 7 7)",
    );
}

#[test]
fn test_r5rs_eof() {
    assert_program_eval_to("(import (scheme r5rs)) (eof-object? (eof-object))", "#t");
}

#[test]
fn test_r5rs_load() {
    assert_program_eval_to("(import (scheme r5rs)) (procedure? load)", "#t");
}

#[test]
fn test_r5rs_interaction_environment() {
    assert_program_eval_to(
        "(import (scheme r5rs)) (eval '(+ 10 20) (interaction-environment))",
        "30",
    );
}
