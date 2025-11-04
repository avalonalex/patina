//! R7RS Section 6.10 - Control features
//!
//! Tests based on chibi-scheme's r7rs-tests.scm
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

use super::common::*;

// 6.10 Control features - apply
#[test]
fn test_apply_basic() {
    assert_eval_to("(apply + (list 3 4))", "7");
}

#[test]
fn test_apply_with_args() {
    assert_eval_to("(apply + 1 2 (list 3 4))", "10");
}

#[test]
fn test_apply_empty_list() {
    assert_eval_to("(apply + (list))", "0");
}

#[test]
fn test_apply_with_lambda() {
    assert_program_eval_to(
        r#"
        (define compose
          (lambda (f g)
            (lambda args
              (f (apply g args)))))
        ((compose - +) 12 75)
        "#,
        "-87",
    );
}

#[test]
fn test_apply_multiply() {
    assert_eval_to("(apply * 2 3 (list 4 5))", "120");
}

// 6.10 Control features - values and call-with-values
#[test]
fn test_values_single() {
    // Single value is returned unwrapped
    assert_eval_to("(values 42)", "42");
}

#[test]
fn test_call_with_values_basic() {
    // Example from R7RS spec
    assert_eval_to(
        "(call-with-values (lambda () (values 4 5)) (lambda (a b) b))",
        "5",
    );
}

#[test]
fn test_call_with_values_add() {
    assert_eval_to(
        "(call-with-values (lambda () (values 1 2 3)) (lambda (a b c) (+ a b c)))",
        "6",
    );
}

#[test]
fn test_call_with_values_primitives() {
    // Example from R7RS: (call-with-values * -) => -1
    // * with no args returns 1, - with one arg returns negation
    assert_eval_to("(call-with-values * -)", "-1");
}

#[test]
fn test_call_with_values_multiple_uses() {
    assert_program_eval_to(
        r#"
        (call-with-values (lambda () (values 1 2))
          (lambda (a b) (+ a b)))
        "#,
        "3",
    );
}
