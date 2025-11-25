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

/// Test scope-based hygiene: macro captures binding from definition site
///
/// This is the classic R7RS hygiene test case where a macro defined inside
/// a let should capture the outer binding of 'x', not the inner one.
///
/// From R7RS Section 4.3: "If a macro transformer inserts a free reference
/// to an identifier, the reference refers to the binding that was visible
/// where the transformer was specified, regardless of any local bindings
/// that surround the use of the macro."
#[test]
fn test_let_syntax_captures_definition_site_binding() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ((x 'outer))
          (let-syntax ((m (syntax-rules () ((m) x))))
            (let ((x 'inner))
              (m))))
        "#,
    );

    assert!(result.is_ok());
    // The macro's 'x' should refer to 'outer' (definition site),
    // not 'inner' (expansion site)
    assert_eq!(result.unwrap().to_string(), "outer");
}

/// Test scope-based hygiene with define wrapper
///
/// Same as above but wrapped in a define to ensure hygiene works
/// across function boundaries.
#[test]
fn test_let_syntax_hygiene_in_define() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define (test-hygiene)
          (let ((x 'outer))
            (let-syntax ((m (syntax-rules () ((m) x))))
              (let ((x 'inner))
                (m)))))
        (test-hygiene)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "outer");
}

/// Test scope-based hygiene with lambda wrapper
///
/// Ensure hygiene works when the macro is defined inside a lambda parameter scope.
#[test]
fn test_let_syntax_hygiene_in_lambda() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        ((lambda (x)
           (let-syntax ((m (syntax-rules () ((m) x))))
             (let ((x 'inner))
               (m))))
         'outer)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "outer");
}

/// Test scope-based hygiene with multiple nested lets
#[test]
fn test_let_syntax_hygiene_multiple_nesting() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ((x 'level1))
          (let ((y 'level2))
            (let-syntax ((m (syntax-rules () ((m) x))))
              (let ((x 'inner))
                (m)))))
        "#,
    );

    assert!(result.is_ok());
    // Should return 'level1, not 'inner
    assert_eq!(result.unwrap().to_string(), "level1");
}

// =============================================================================
// Underscore Literal Tests
// =============================================================================
//
// R7RS Section 4.3.2: When `_` appears in the literals list, it should only
// match the literal symbol `_`, not act as a wildcard pattern.

/// Test underscore as wildcard (default behavior when not in literals)
#[test]
fn test_underscore_as_wildcard() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax count-args
          (syntax-rules ()
            ((_ a) 1)
            ((_ a b) 2)
            ((_ a b c) 3)))

        (list (count-args x) (count-args x y) (count-args x y z))
        "#,
    );

    assert!(result.is_ok());
    // _ should match any symbol in macro keyword position
    assert_eq!(result.unwrap().to_string(), "(1 2 3)");
}

/// Test underscore as literal (when explicitly in literals list)
///
/// R7RS: When `_` is in the literals list, `_` in patterns should only
/// match the literal symbol `_`, not act as a wildcard.
#[test]
fn test_underscore_as_literal() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax count-to-2_
          (syntax-rules (_)
            ((_) 0)
            ((_ _) 1)
            ((_ _ _) 2)
            ((x . y) 'fail)))

        (list (count-to-2_ _ _)
              (count-to-2_)
              (count-to-2_ a b)
              (count-to-2_ a b c d))
        "#,
    );

    assert!(result.is_ok());
    // Pattern (_ _ _) should only match literal _, not a and b
    assert_eq!(result.unwrap().to_string(), "(2 0 fail fail)");
}

// =============================================================================
// Ellipsis Escape Tests
// =============================================================================
//
// R7RS Section 4.3.2: "(... template)" is an escape form that produces
// "template" with ellipsis treated as a regular symbol.

/// Test basic ellipsis escape: (... ...) produces literal ...
#[test]
fn test_ellipsis_escape_basic() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-ellipsis
          (syntax-rules ()
            ((_) (quote (... ...)))))

        (make-ellipsis)
        "#,
    );

    assert!(result.is_ok());
    // (... ...) should produce just ...
    assert_eq!(result.unwrap().to_string(), "...");
}

/// Test ellipsis escape preserves pattern variable substitution
#[test]
fn test_ellipsis_escape_with_pattern_variable() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax elli-esc-1
          (syntax-rules ()
            ((_ x) (quote (... (x ...))))))

        (elli-esc-1 foo)
        "#,
    );

    assert!(result.is_ok());
    // x should be substituted, but ... remains literal
    assert_eq!(result.unwrap().to_string(), "(foo ...)");
}

/// Test ellipsis escape with multiple pattern variables
#[test]
fn test_ellipsis_escape_multiple_vars() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax elli-esc-2
          (syntax-rules ()
            ((_ x y) (quote (... (... x y))))))

        (elli-esc-2 bar baz)
        "#,
    );

    assert!(result.is_ok());
    // Both x and y should be substituted, ... remains literal
    assert_eq!(result.unwrap().to_string(), "(... bar baz)");
}

/// Test that ellipsis escape produces proper Symbol values
#[test]
fn test_ellipsis_escape_produces_symbol() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-ellipsis
          (syntax-rules ()
            ((_) (quote (... ...)))))

        (equal? (make-ellipsis) '...)
        "#,
    );

    assert!(result.is_ok());
    // Should be equal to '...
    assert_eq!(result.unwrap().to_string(), "#t");
}

/// Test ellipsis escape in list produces equal? results
#[test]
fn test_ellipsis_escape_equal_list() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax elli-with-var
          (syntax-rules ()
            ((_ x) (quote (... (x ...))))))

        (equal? (elli-with-var 100) '(100 ...))
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "#t");
}

// =============================================================================
// Nested Macro Definition Tests
// =============================================================================
//
// R7RS: Macros that generate other macros using ellipsis escape should work
// correctly. The inner macro's ellipsis should be preserved as a literal
// symbol so it can be recognized when the inner macro is compiled.

/// Test simple nested macro definition
///
/// A macro that generates another macro with no ellipsis in the inner template.
#[test]
fn test_nested_macro_simple() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax gen-const-macro
          (syntax-rules ()
            ((gen-const-macro name value)
             (... (define-syntax name
                    (syntax-rules ()
                      ((name) value)))))))

        (gen-const-macro answer 42)
        (answer)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "42");
}

/// Test nested macro with ellipsis in inner template
///
/// This is the classic "be-like-begin" example from chibi-scheme tests.
/// The outer macro generates an inner macro that uses ellipsis.
#[test]
fn test_nested_macro_with_ellipsis() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax be-like-begin
          (syntax-rules ()
            ((be-like-begin name)
             (define-syntax name
               (... (syntax-rules ()
                      ((name expr ...)
                       (begin expr ...))))))))

        (be-like-begin sequence)
        (sequence 1 2 3 4)
        "#,
    );

    assert!(result.is_ok());
    // sequence should work like begin, returning the last value
    assert_eq!(result.unwrap().to_string(), "4");
}

/// Test nested macro generating a list macro
#[test]
fn test_nested_macro_listify() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax gen-list-macro
          (syntax-rules ()
            ((gen-list-macro name)
             (... (define-syntax name
                    (syntax-rules ()
                      ((name x ...)
                       (list x ...))))))))

        (gen-list-macro make-list)
        (make-list 1 2 3 4 5)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "(1 2 3 4 5)");
}
