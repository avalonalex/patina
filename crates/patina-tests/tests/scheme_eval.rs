//! Tests for R7RS (scheme eval) library (Section 6.12)

use patina_interpreter::TreeWalkInterpreter;

fn eval_program(code: &str) -> String {
    let interp = TreeWalkInterpreter::new_tree_walker();
    match interp.eval_program(code) {
        Ok(v) => interp.display_tagged(v),
        Err(e) => format!("ERROR: {}", e),
    }
}

// =============================================================================
// Basic environment tests
// =============================================================================

#[test]
fn test_environment_returns_environment() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (environment '(scheme base))
    "#,
    );
    assert!(
        result.contains("environment"),
        "Expected environment, got: {}",
        result
    );
}

#[test]
fn test_environment_empty() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (environment)
    "#,
    );
    assert!(
        result.contains("environment"),
        "Expected environment, got: {}",
        result
    );
}

// =============================================================================
// Basic eval tests
// =============================================================================

#[test]
fn test_eval_simple_arithmetic() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(* 7 3) (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "21");
}

#[test]
fn test_eval_addition() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(+ 1 2 3 4 5) (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "15");
}

#[test]
fn test_eval_expt() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(expt 2 10) (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "1024");
}

// =============================================================================
// Eval with lambda
// =============================================================================

#[test]
fn test_eval_lambda() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (let ((f (eval '(lambda (x) (* x x))
                       (environment '(scheme base)))))
          (f 5))
    "#,
    );
    assert_eq!(result, "25");
}

#[test]
fn test_eval_lambda_multiple_args() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (let ((f (eval '(lambda (f x) (f x x))
                       (environment '(scheme base)))))
          (f + 10))
    "#,
    );
    assert_eq!(result, "20");
}

// =============================================================================
// Multiple libraries
// =============================================================================

#[test]
fn test_eval_with_scheme_inexact() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (inexact (eval '(sin 0) (environment '(scheme inexact))))
    "#,
    );
    assert_eq!(result, "0.0");
}

#[test]
fn test_eval_with_multiple_libraries() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(+ (expt 2 10) (inexact (sin 0)))
              (environment '(scheme base) '(scheme inexact)))
    "#,
    );
    assert_eq!(result, "1024.0");
}

// =============================================================================
// Immutability tests
// =============================================================================

#[test]
fn test_eval_define_in_immutable_env_errors() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(define foo 32) (environment '(scheme base)))
    "#,
    );
    assert!(
        result.contains("ERROR"),
        "Expected error for define in immutable env, got: {}",
        result
    );
    assert!(
        result.contains("immutable"),
        "Expected 'immutable' in error message, got: {}",
        result
    );
}

// =============================================================================
// Complex expressions
// =============================================================================

#[test]
fn test_eval_if_expression() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(if #t 1 2) (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "1");
}

#[test]
fn test_eval_let_expression() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(let ((x 10) (y 20)) (+ x y)) (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "30");
}

#[test]
fn test_eval_nested_expressions() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(let ((square (lambda (x) (* x x))))
                 (+ (square 3) (square 4)))
              (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "25");
}

// =============================================================================
// List operations in eval
// =============================================================================

#[test]
fn test_eval_list_operations() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(car (cons 1 2)) (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "1");
}

#[test]
fn test_eval_map() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(map (lambda (x) (* x 2)) '(1 2 3)) (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "(2 4 6)");
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn test_eval_quote() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(quote (a b c)) (environment '(scheme base)))
    "#,
    );
    assert_eq!(result, "(a b c)");
}

#[test]
fn test_eval_empty_environment_if() {
    // Special forms should work even in empty environment
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(if #t 42 0) (environment))
    "#,
    );
    assert_eq!(result, "42");
}

#[test]
fn test_environment_type_error() {
    let result = eval_program(
        r#"
        (import (scheme eval))
        (eval '(+ 1 2) 42)
    "#,
    );
    assert!(
        result.contains("ERROR"),
        "Expected error for non-environment, got: {}",
        result
    );
}

// =============================================================================
// R5RS environment tests (null-environment and scheme-report-environment)
// =============================================================================

#[test]
fn test_null_environment_returns_environment() {
    let result = eval_program(
        r#"
        (import (scheme r5rs))
        (null-environment 5)
    "#,
    );
    assert!(
        result.contains("environment"),
        "Expected environment, got: {}",
        result
    );
}

#[test]
fn test_null_environment_only_accepts_version_5() {
    let result = eval_program(
        r#"
        (import (scheme r5rs))
        (null-environment 6)
    "#,
    );
    assert!(
        result.contains("ERROR"),
        "Expected error for unsupported version, got: {}",
        result
    );
}

#[test]
fn test_null_environment_has_if() {
    // null-environment should have syntactic keywords like if
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(if #t 1 2) (null-environment 5))
    "#,
    );
    assert_eq!(result, "1");
}

#[test]
fn test_null_environment_has_lambda() {
    // null-environment should have lambda
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '((lambda (x) x) 42) (null-environment 5))
    "#,
    );
    assert_eq!(result, "42");
}

#[test]
fn test_null_environment_has_quote() {
    // null-environment should have quote
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(quote (a b c)) (null-environment 5))
    "#,
    );
    assert_eq!(result, "(a b c)");
}

#[test]
fn test_null_environment_no_procedures() {
    // null-environment should NOT have procedures like +
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(+ 1 2) (null-environment 5))
    "#,
    );
    assert!(
        result.contains("ERROR") || result.contains("Undefined"),
        "Expected error for + in null-environment, got: {}",
        result
    );
}

#[test]
fn test_scheme_report_environment_returns_environment() {
    let result = eval_program(
        r#"
        (import (scheme r5rs))
        (scheme-report-environment 5)
    "#,
    );
    assert!(
        result.contains("environment"),
        "Expected environment, got: {}",
        result
    );
}

#[test]
fn test_scheme_report_environment_only_accepts_version_5() {
    let result = eval_program(
        r#"
        (import (scheme r5rs))
        (scheme-report-environment 7)
    "#,
    );
    assert!(
        result.contains("ERROR"),
        "Expected error for unsupported version, got: {}",
        result
    );
}

#[test]
fn test_scheme_report_environment_has_arithmetic() {
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(+ 1 2 3) (scheme-report-environment 5))
    "#,
    );
    assert_eq!(result, "6");
}

#[test]
fn test_scheme_report_environment_has_list_operations() {
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(car (cons 1 2)) (scheme-report-environment 5))
    "#,
    );
    assert_eq!(result, "1");
}

#[test]
fn test_scheme_report_environment_has_map() {
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(map (lambda (x) (* x 2)) '(1 2 3)) (scheme-report-environment 5))
    "#,
    );
    assert_eq!(result, "(2 4 6)");
}

#[test]
fn test_scheme_report_environment_has_let() {
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(let ((x 10)) (* x x)) (scheme-report-environment 5))
    "#,
    );
    assert_eq!(result, "100");
}

#[test]
fn test_scheme_report_environment_has_predicates() {
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(list (number? 42) (string? "hello") (null? '()))
              (scheme-report-environment 5))
    "#,
    );
    assert_eq!(result, "(#t #t #t)");
}

#[test]
fn test_scheme_report_environment_has_string_ops() {
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(string-append "hello" " " "world") (scheme-report-environment 5))
    "#,
    );
    assert_eq!(result, "\"hello world\"");
}

#[test]
fn test_scheme_report_environment_immutable() {
    // scheme-report-environment returns an immutable environment
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(define x 10) (scheme-report-environment 5))
    "#,
    );
    assert!(
        result.contains("ERROR"),
        "Expected error for define in immutable env, got: {}",
        result
    );
}

#[test]
fn test_null_environment_immutable() {
    // null-environment returns an immutable environment
    let result = eval_program(
        r#"
        (import (scheme r5rs) (scheme eval))
        (eval '(define x 10) (null-environment 5))
    "#,
    );
    assert!(
        result.contains("ERROR"),
        "Expected error for define in immutable env, got: {}",
        result
    );
}
