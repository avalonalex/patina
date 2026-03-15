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
