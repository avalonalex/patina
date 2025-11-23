//! Tests for case-lambda special form
//!
//! Tests the (scheme case-lambda) library implementation.
//!
//! R7RS Reference: Section 4.2.9 - Case-lambda

mod common;
use common::*;

// ============================================================================
// Basic Case-Lambda Tests
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_zero_args() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            (() 'zero)))
        (f)
        "#,
        "zero",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_one_arg() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            ((x) x)))
        (f 42)
        "#,
        "42",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_two_args() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            ((x y) (cons x y))))
        (f 1 2)
        "#,
        "(1 . 2)",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_multiple_clauses() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            (() 'zero)
            ((x) x)
            ((x y) (cons x y))))
        (list (f) (f 1) (f 1 2))
        "#,
        "(zero 1 (1 . 2))",
    );
}

// ============================================================================
// Variadic Case-Lambda Tests
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_variadic_only() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            (args args)))
        (f 1 2 3)
        "#,
        "(1 2 3)",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_variadic_zero_args() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            (args args)))
        (f)
        "#,
        "()",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_mixed_fixed_and_variadic() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            ((x . rest) (cons x rest))))
        (f 1 2 3)
        "#,
        "(1 2 3)",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_multiple_with_variadic() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            (() 'zero)
            ((x) (list 'one x))
            ((x y) (list 'two x y))
            ((x y . z) (list 'more x y z))))
        (list (f) (f 1) (f 1 2) (f 1 2 3) (f 1 2 3 4))
        "#,
        "(zero (one 1) (two 1 2) (more 1 2 (3)) (more 1 2 (3 4)))",
    );
}

// ============================================================================
// R7RS Examples from Chibi Test Suite
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_chibi_any_arity() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define any-arity
          (case-lambda
            (() 'zero)
            ((x) x)
            ((x y) (cons x y))
            ((x y z) (list x y z))
            (args (cons 'many args))))
        (list (any-arity)
              (any-arity 1)
              (any-arity 1 2)
              (any-arity 1 2 3)
              (any-arity 1 2 3 4))
        "#,
        "(zero 1 (1 . 2) (1 2 3) (many 1 2 3 4))",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_chibi_rest_arity() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define rest-arity
          (case-lambda
            (() '(zero))
            ((x) (list 'one x))
            ((x y) (list 'two x y))
            ((x y . z) (list 'more x y z))))
        (list (rest-arity)
              (rest-arity 1)
              (rest-arity 1 2)
              (rest-arity 1 2 3)
              (rest-arity 1 2 3 4))
        "#,
        "((zero) (one 1) (two 1 2) (more 1 2 (3)) (more 1 2 (3 4)))",
    );
}

// ============================================================================
// Closure and Environment Tests
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_captures_environment() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define x 10)
        (define f
          (case-lambda
            (() x)
            ((y) (+ x y))))
        (list (f) (f 5))
        "#,
        "(10 15)",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_nested_closures() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define make-counter
          (lambda (n)
            (case-lambda
              (() n)
              ((x) (set! n (+ n x)) n))))
        (define c (make-counter 0))
        (list (c) (c 5) (c 3) (c))
        "#,
        "(0 5 8 8)",
    );
}

// ============================================================================
// Higher-Order Function Tests
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_as_argument() {
    assert_program_eval_to(        
        r#"
        (import (scheme case-lambda))
        (define apply-twice
          (lambda (f)
            (list (f 1) (f 1 2))))
        (apply-twice
          (case-lambda
            ((x) (* x x))
            ((x y) (+ x y))))
        "#,
        "(1 3)",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_returned_from_function() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define make-adder
          (lambda (n)
            (case-lambda
              ((x) (+ n x))
              ((x y) (+ n x y)))))
        (define add5 (make-adder 5))
        (list (add5 3) (add5 2 4))
        "#,
        "(8 11)",
    );
}

// ============================================================================
// Error Cases
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_no_matching_clause() {
    assert_program_eval_error(
        r#"
        (import (scheme case-lambda))        
        (define f
          (case-lambda
            ((x) x)
            ((x y) (cons x y))))
        (f 1 2 3)
        "#,
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_empty_clause_list() {
    assert_program_eval_error("(case-lambda)");
}

// ============================================================================
// Complex Examples
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_fibonacci() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define fib
          (case-lambda
            (() (fib 10))
            ((n) (if (<= n 1)
                     n
                     (+ (fib (- n 1)) (fib (- n 2)))))))
        (fib 7)
        "#,
        "13",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_list_operations() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define my-list
          (case-lambda
            (() '())
            ((x) (list x))
            ((x y) (list x y))
            (args args)))
        (list (my-list)
              (my-list 1)
              (my-list 1 2)
              (my-list 1 2 3 4 5))
        "#,
        "(() (1) (1 2) (1 2 3 4 5))",
    );
}

// ============================================================================
// Import Test
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_library_import() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define f
          (case-lambda
            (() 'imported)
            ((x) x)))
        (f)
        "#,
        "imported",
    );
}

// ============================================================================
// Tail Call Optimization Tests
// ============================================================================

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_tail_recursion() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define countdown
          (case-lambda
            (() (countdown 100))
            ((n) (if (= n 0)
                     'done
                     (countdown (- n 1))))))
        (countdown)
        "#,
        "done",
    );
}

#[test]
#[ignore = "need to properly support import"]
fn test_case_lambda_tail_call_in_different_clauses() {
    assert_program_eval_to(
        r#"
        (import (scheme case-lambda))
        (define mutual-rec
          (case-lambda
            ((n) (if (= n 0) 'done (mutual-rec n 0)))
            ((n acc) (if (= n 0)
                         acc
                         (mutual-rec (- n 1) (+ acc 1))))))
        (mutual-rec 1000)
        "#,
        "1000",
    );
}
