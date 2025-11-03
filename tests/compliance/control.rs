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
