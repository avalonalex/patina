//! Tests for define-values macro (R7RS Section 5.3.3)
//!
//! These tests are designed to identify issues with the current define-values
//! implementation in lib/scheme/base-extras.scm

mod common;
use common::*;

// Test case 1: No variables - evaluate for side effects
#[test]
fn test_define_values_no_vars() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values () (values))
          'ok)
        "#,
        "ok",
    );
}

// Test case 2: Single variable
#[test]
fn test_define_values_single_var() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (x) (values 1))
          x)
        "#,
        "1",
    );
}

// Test case 3: Improper list (variable after dot) - collect remaining values
#[test]
fn test_define_values_improper_list() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values x (values 1 2))
          (apply + x))
        "#,
        "3",
    );
}

// Test case 4: Two variables
#[test]
fn test_define_values_two_vars() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (x y) (values 1 2))
          (+ x y))
        "#,
        "3",
    );
}

// Test case 5: Three variables
#[test]
fn test_define_values_three_vars() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (x y z) (values 1 2 3))
          (+ x y z))
        "#,
        "6",
    );
}

// Test case 6: Dotted list - first few variables, rest in final variable
#[test]
fn test_define_values_dotted_list() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (x y . z) (values 1 2 3 4))
          (+ x y (car z) (cadr z)))
        "#,
        "10",
    );
}

// Additional tests to understand the macro expansion

// Test 7: Two variables - verify both are correctly assigned
#[test]
fn test_define_values_two_vars_separate_access() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (x y) (values 10 20))
          (list x y))
        "#,
        "(10 20)",
    );
}

// Test 8: Three variables - verify all are correctly assigned
#[test]
fn test_define_values_three_vars_separate_access() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (x y z) (values 10 20 30))
          (list x y z))
        "#,
        "(10 20 30)",
    );
}

// Test 9: Four variables - tests the ellipsis expansion
#[test]
fn test_define_values_four_vars() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (a b c d) (values 1 2 3 4))
          (list a b c d))
        "#,
        "(1 2 3 4)",
    );
}

// Test 10: Check that values are evaluated correctly
#[test]
fn test_define_values_with_computation() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (x y) (values (+ 1 2) (* 3 4)))
          (+ x y))
        "#,
        "15",
    );
}

// Test 11: No variables case should not fail even if expression returns values
#[test]
fn test_define_values_no_vars_with_values() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values () (values 1 2 3))
          42)
        "#,
        "42",
    );
}

// Test 12: Single variable should work with single value
#[test]
fn test_define_values_single_var_single_value() {
    assert_program_eval_to(
        r#"
        (let ()
          (define-values (x) 42)
          x)
        "#,
        "42",
    );
}
