//! Tests for desugarer behavior with macro-expanded code
//!
//! These tests explore what happens when we try to desugar macro-expanded code,
//! helping us understand the CoreExpr integration issue.
//!
//! NOTE: These are unit tests for the desugarer only. Full integration tests
//! (including macro expansion) are in patina-tests.

use patina_core::{SharedHeap, TaggedValue};
use patina_frontend::{Desugarer, Parser};
use patina_ir::CoreExprKind;

/// Helper: Parse code string to TaggedValue, returning the heap for desugar_tagged
fn parse_tv(code: &str) -> (TaggedValue, SharedHeap) {
    let mut parser = Parser::new(code).expect("Parser creation failed");
    let tv = parser.parse().expect("Parse failed");
    (tv, parser.heap().clone())
}

#[test]
fn test_desugar_simple_quote() {
    let (tv, heap) = parse_tv("(quote a)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    assert!(result.is_ok(), "Should desugar simple quote: {:?}", result);
    println!("Quote desugared to: {:?}", result.unwrap());
}

#[test]
fn test_desugar_simple_lambda() {
    let (tv, heap) = parse_tv("(lambda (x) x)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    assert!(result.is_ok(), "Should desugar simple lambda: {:?}", result);
    println!("Lambda desugared to: {:?}", result.unwrap());
}

#[test]
fn test_desugar_lambda_with_if() {
    let (tv, heap) = parse_tv("(lambda (x) (if x 1 2))");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

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
    let (tv, heap) = parse_tv("(or #f #t)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    // Actually SUCCEEDS - treats 'or' as a function application
    assert!(result.is_ok(), "Desugarer treats 'or' as application");
    println!("Unexpanded 'or' desugars to: {:?}", result.unwrap());
    // Result: App { func: Var("or"), args: [Literal(Boolean(false)), Literal(Boolean(true))] }
    // This will FAIL at evaluation time when 'or' isn't found as a procedure!
}

#[test]
fn test_desugar_unexpanded_let() {
    // Should succeed but treat 'let' as a function application
    let (tv, heap) = parse_tv("(let ((x 1)) x)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    assert!(result.is_ok(), "Desugarer treats 'let' as application");
    println!("Unexpanded 'let' desugars to: {:?}", result.unwrap());
    // Result: App { func: Var("let"), args: [...] }
}

#[test]
fn test_desugar_manually_expanded_or() {
    // Manually write what 'or' expands to: (if test test else)
    // (or a b) => (let ((tmp a)) (if tmp tmp b))
    let (tv, heap) = parse_tv("((lambda (tmp) (if tmp tmp #t)) #f)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

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
    let (tv, heap) = parse_tv("((lambda (x) x) 1)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

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
    let (tv, heap) = parse_tv("(lambda (x y) (if (if x #t #f) (begin (set! x 1) y) x))");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    println!("Nested core forms result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar nested core forms: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_define_simple() {
    let (tv, heap) = parse_tv("(define x 42)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    println!("Simple define result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar simple define: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_define_function() {
    let (tv, heap) = parse_tv("(define (f x) x)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    println!("Function define result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar function define: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_begin() {
    let (tv, heap) = parse_tv("(begin 1 2 3)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    println!("Begin result: {:?}", result);
    assert!(result.is_ok(), "Should desugar begin: {:?}", result.err());
}

#[test]
fn test_desugar_quasiquote() {
    let (tv, heap) = parse_tv("`(a ,b ,@c)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    println!("Quasiquote result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar quasiquote: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_application() {
    let (tv, heap) = parse_tv("(+ 1 2 3)");
    let desugarer = Desugarer::new();
    let result = desugarer.desugar_tagged(tv, &heap);

    println!("Application result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar application: {:?}",
        result.err()
    );
}

// =========================================================================
// Desugarer Macro Tests - define-syntax compiles immediately
// =========================================================================

use patina_runtime::Environment;
use std::rc::Rc;

/// Test that desugarer returns Literal(Unspecified) for define-syntax
/// (macro is compiled immediately and installed in environment)
#[test]
fn test_desugarer_returns_unspecified_for_define_syntax() {
    let env = Rc::new(Environment::new());
    let heap = env.heap().clone();
    let mut parser = Parser::new_with_heap(
        "(define-syntax foo (syntax-rules () ((foo x) x)))",
        heap.clone(),
    )
    .expect("Parser creation failed");
    let tv = parser.parse().expect("Parse failed");

    let desugarer = Desugarer::with_env(env);
    let result = desugarer
        .desugar_tagged(tv, &heap)
        .expect("Should desugar successfully");

    // Should be Literal(Unspecified) - macro is compiled at desugar time
    assert!(
        matches!(&result.kind, CoreExprKind::Literal(val) if *val == TaggedValue::UNSPECIFIED),
        "Expected Literal(Unspecified), got {:?}",
        result
    );
}

/// Test that desugarer installs macro in environment
#[test]
fn test_desugarer_installs_macro_in_environment() {
    let env = Rc::new(Environment::new());
    let heap = env.heap().clone();
    let mut parser = Parser::new_with_heap(
        "(define-syntax foo (syntax-rules () ((foo x) x)))",
        heap.clone(),
    )
    .expect("Parser creation failed");
    let tv = parser.parse().expect("Parse failed");

    let desugarer = Desugarer::with_env(env.clone());
    let _ = desugarer
        .desugar_tagged(tv, &heap)
        .expect("Should desugar successfully");

    // Macro should now be in the environment
    let macro_tv = env.get("foo");
    assert!(
        macro_tv.is_some(),
        "Macro 'foo' should be installed in environment"
    );
    assert!(
        heap.borrow().is_macro(macro_tv.unwrap()),
        "foo should be a Macro value"
    );
}
