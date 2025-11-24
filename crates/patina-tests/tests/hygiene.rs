//! Tests for macro hygiene
//!
//! These tests verify that the macro system properly implements hygienic renaming
//! to prevent macro-introduced identifiers from capturing user bindings.
//!
//! Related issues:
//! - https://github.com/avalonalex/patina/issues/12

use patina_interpreter::TreeWalkInterpreter;

/// Test that macro-introduced identifiers don't capture user bindings
///
/// Issue #12: https://github.com/avalonalex/patina/issues/12
#[test]
#[ignore] // TODO: Still has issues with some cases - needs more work
fn test_macro_introduced_temp_variable() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Define a macro that introduces a 'temp' variable
    let result = interp.eval_program(
        r#"
        (define-syntax my-let
          (syntax-rules ()
            ((my-let x body)
             (let ((temp x)) body))))

        (let ((temp 5))
          (my-let 10 temp))
        "#,
    );

    // Should return 10 (the value of the macro argument)
    // NOT 5 (the value of the user's temp binding)
    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "10");
}

/// Test hygiene with nested lets
#[test]
fn test_nested_let_hygiene() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax my-swap
          (syntax-rules ()
            ((my-swap a b)
             (let ((temp a))
               (set! a b)
               (set! b temp)))))

        (let ((x 1)
              (y 2)
              (temp 999))
          (my-swap x y)
          (list x y temp))
        "#,
    );

    assert!(result.is_ok());
    // x and y should be swapped, temp should still be 999
    assert_eq!(result.unwrap().to_string(), "(2 1 999)");
}

/// Test that macro-introduced 'if' doesn't capture user 'if'
#[test]
fn test_special_form_not_captured() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax my-cond
          (syntax-rules ()
            ((my-cond test then-clause)
             (if test then-clause #f))))

        (let ((if 'captured))
          (my-cond #t 'success))
        "#,
    );

    assert!(result.is_ok());
    // Should return 'success, not try to use 'if' as a procedure
    assert_eq!(result.unwrap().to_string(), "success");
}

/// Test hygiene with multiple macro-introduced bindings
#[test]
fn test_multiple_introduced_bindings() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax with-temps
          (syntax-rules ()
            ((with-temps expr)
             (let ((temp1 1)
                   (temp2 2))
               (+ temp1 temp2 expr)))))

        (let ((temp1 100)
              (temp2 200))
          (with-temps 3))
        "#,
    );

    assert!(result.is_ok());
    // Should return 1 + 2 + 3 = 6, not use the user's temp1/temp2
    assert_eq!(result.unwrap().to_string(), "6");
}

/// Test that pattern variables from the call site are preserved
#[test]
fn test_pattern_variable_preservation() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax apply-twice
          (syntax-rules ()
            ((apply-twice f x)
             (f (f x)))))

        (define (double x) (* 2 x))
        (apply-twice double 3)
        "#,
    );

    assert!(result.is_ok());
    // Should return 12 (double applied twice to 3)
    assert_eq!(result.unwrap().to_string(), "12");
}

/// Test hygiene with lambda
#[test]
fn test_lambda_hygiene() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-adder
          (syntax-rules ()
            ((make-adder n)
             (lambda (x) (+ x n)))))

        (let ((x 100)
              (n 200))
          ((make-adder 5) 3))
        "#,
    );

    assert!(result.is_ok());
    // Should return 8 (3 + 5), using the macro's n, not the outer bindings
    assert_eq!(result.unwrap().to_string(), "8");
}

/// Test that quoted symbols in templates are not renamed
#[test]
#[ignore] // TODO: Quoted symbols are being renamed incorrectly
fn test_quoted_symbols_not_renamed() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-symbol
          (syntax-rules ()
            ((make-symbol)
             'temp)))

        (eq? (make-symbol) 'temp)
        "#,
    );

    assert!(result.is_ok());
    // The quoted 'temp should remain as the symbol temp
    assert_eq!(result.unwrap().to_string(), "#t");
}

/// Test hygiene doesn't break recursive macros
#[test]
#[ignore = "Stack overflow - needs investigation"]
fn test_recursive_macro_hygiene() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax countdown
          (syntax-rules ()
            ((countdown 0) 0)
            ((countdown n)
             (let ((temp n))
               (if (> temp 0)
                   (countdown (- temp 1))
                   temp)))))

        (countdown 3)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "0");
}
