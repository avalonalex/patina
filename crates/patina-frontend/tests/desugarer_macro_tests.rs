//! Tests for desugarer behavior with macro-expanded code
//!
//! These tests explore what happens when we try to desugar macro-expanded code,
//! helping us understand the CoreExpr integration issue.
//!
//! NOTE: These are unit tests for the desugarer only. Full integration tests
//! (including macro expansion) are in patina-tests.

use patina_frontend::{Desugarer, Parser};
use patina_runtime::Value;

/// Helper: Parse code string to Value
fn parse(code: &str) -> Value {
    let mut parser = Parser::new(code).expect("Parser creation failed");
    parser.parse().expect("Parse failed")
}

#[test]
fn test_desugar_simple_quote() {
    let code = "(quote a)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    assert!(result.is_ok(), "Should desugar simple quote: {:?}", result);
    println!("Quote desugared to: {:?}", result.unwrap());
}

#[test]
fn test_desugar_simple_lambda() {
    let code = "(lambda (x) x)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    assert!(result.is_ok(), "Should desugar simple lambda: {:?}", result);
    println!("Lambda desugared to: {:?}", result.unwrap());
}

#[test]
fn test_desugar_lambda_with_if() {
    let code = "(lambda (x) (if x 1 2))";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    assert!(
        result.is_ok(),
        "Should desugar lambda with if: {:?}",
        result
    );
    println!("Lambda with if desugared to: {:?}", result.unwrap());
}

#[test]
fn test_desugar_unexpanded_or() {
    // This is what happens if we try to desugar BEFORE macro expansion
    // The desugarer treats 'or' as a regular application!
    let code = "(or #f #t)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    // Actually SUCCEEDS - treats 'or' as a function application
    assert!(result.is_ok(), "Desugarer treats 'or' as application");
    println!("Unexpanded 'or' desugars to: {:?}", result.unwrap());
    // Result: App { func: Var("or"), args: [Literal(Boolean(false)), Literal(Boolean(true))] }
    // This will FAIL at evaluation time when 'or' isn't found as a procedure!
}

#[test]
fn test_desugar_unexpanded_let() {
    // Should succeed but treat 'let' as a function application
    let code = "(let ((x 1)) x)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    assert!(result.is_ok(), "Desugarer treats 'let' as application");
    println!("Unexpanded 'let' desugars to: {:?}", result.unwrap());
    // Result: App { func: Var("let"), args: [...] }
}

#[test]
fn test_desugar_manually_expanded_or() {
    // Manually write what 'or' expands to: (if test test else)
    // (or a b) => (let ((tmp a)) (if tmp tmp b))
    let code = "((lambda (tmp) (if tmp tmp #t)) #f)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    println!("Manually expanded 'or' result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar manually expanded 'or': {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_manually_expanded_let() {
    // Manually write what 'let' expands to: ((lambda (vars...) body...) values...)
    // (let ((x 1)) x) => ((lambda (x) x) 1)
    let code = "((lambda (x) x) 1)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    println!("Manually expanded 'let' result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar manually expanded 'let': {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_nested_core_forms() {
    // No macros - all core forms
    let code = "(lambda (x y) (if (if x #t #f) (begin (set! x 1) y) x))";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    println!("Nested core forms result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar nested core forms: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_define_simple() {
    let code = "(define x 42)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    println!("Simple define result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar simple define: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_define_function() {
    let code = "(define (f x) x)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    println!("Function define result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar function define: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_begin() {
    let code = "(begin 1 2 3)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    println!("Begin result: {:?}", result);
    assert!(result.is_ok(), "Should desugar begin: {:?}", result.err());
}

#[test]
fn test_desugar_quasiquote() {
    let code = "`(a ,b ,@c)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    println!("Quasiquote result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar quasiquote: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_application() {
    let code = "(+ 1 2 3)";
    let value = parse(code);

    let desugarer = Desugarer::new();
    let result = desugarer.desugar(&value);

    println!("Application result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar application: {:?}",
        result.err()
    );
}
