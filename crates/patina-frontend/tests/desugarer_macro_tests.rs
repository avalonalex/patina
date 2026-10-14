//! Tests for desugarer behavior with macro-expanded code
//!
//! These tests explore what happens when we try to desugar macro-expanded code,
//! helping us understand the CoreExpr integration issue.
//!
//! NOTE: These are unit tests for the desugarer only. Full integration tests
//! (including macro expansion) are in patina-tests.

use patina_core::{NoGcScope, SharedHeap, TaggedValue, new_shared_heap};
use patina_frontend::{DesugarError, Desugarer, Parser};
use patina_ir::{CoreExpr, CoreExprKind};
use patina_runtime::Environment;
use std::rc::Rc;

/// Helper: An environment holding the syntactic keywords, and the heap its
/// values live on
fn seeded() -> (Rc<Environment>, SharedHeap) {
    let heap = new_shared_heap();
    let env = Rc::new(Environment::with_heap(&heap));
    patina_runtime::stdlib::seed_core_syntax(&env, &heap);
    (env, heap)
}

/// Helper: Desugar `code` with a desugarer whose environment holds the
/// syntactic keywords
fn desugar(code: &str) -> Result<CoreExpr, DesugarError> {
    let (env, heap) = seeded();
    desugar_with(&Desugarer::with_env(env), &heap, code)
}

/// Helper: Desugar `code` with `desugarer`, reading it on `heap`, the heap
/// of the desugarer's environment
fn desugar_with(
    desugarer: &Desugarer,
    heap: &SharedHeap,
    code: &str,
) -> Result<CoreExpr, DesugarError> {
    let mut parser = Parser::new(code).expect("Parser creation failed");
    NoGcScope::with_cx(heap, |cx| {
        let form = parser.parse(cx).expect("Parse failed");
        desugarer.desugar_tagged(form, cx)
    })
}

#[test]
fn test_desugar_simple_quote() {
    let result = desugar("(quote a)");

    assert!(result.is_ok(), "Should desugar simple quote: {:?}", result);
    println!("Quote desugared to: {:?}", result.unwrap());
}

#[test]
fn test_desugar_simple_lambda() {
    let result = desugar("(lambda (x) x)");

    assert!(result.is_ok(), "Should desugar simple lambda: {:?}", result);
    println!("Lambda desugared to: {:?}", result.unwrap());
}

#[test]
fn test_desugar_lambda_with_if() {
    let result = desugar("(lambda (x) (if x 1 2))");

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
    let result = desugar("(or #f #t)");

    // Actually SUCCEEDS - treats 'or' as a function application
    assert!(result.is_ok(), "Desugarer treats 'or' as application");
    println!("Unexpanded 'or' desugars to: {:?}", result.unwrap());
    // Result: App { func: Var("or"), args: [Literal(Boolean(false)), Literal(Boolean(true))] }
    // This will FAIL at evaluation time when 'or' isn't found as a procedure!
}

#[test]
fn test_desugar_unexpanded_let() {
    // Should succeed but treat 'let' as a function application
    let result = desugar("(let ((x 1)) x)");

    assert!(result.is_ok(), "Desugarer treats 'let' as application");
    println!("Unexpanded 'let' desugars to: {:?}", result.unwrap());
    // Result: App { func: Var("let"), args: [...] }
}

#[test]
fn test_desugar_manually_expanded_or() {
    // Manually write what 'or' expands to: (if test test else)
    // (or a b) => (let ((tmp a)) (if tmp tmp b))
    let result = desugar("((lambda (tmp) (if tmp tmp #t)) #f)");

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
    let result = desugar("((lambda (x) x) 1)");

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
    let result = desugar("(lambda (x y) (if (if x #t #f) (begin (set! x 1) y) x))");

    println!("Nested core forms result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar nested core forms: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_define_simple() {
    let result = desugar("(define x 42)");

    println!("Simple define result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar simple define: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_define_function() {
    let result = desugar("(define (f x) x)");

    println!("Function define result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar function define: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_begin() {
    let result = desugar("(begin 1 2 3)");

    println!("Begin result: {:?}", result);
    assert!(result.is_ok(), "Should desugar begin: {:?}", result.err());
}

#[test]
fn test_desugar_quasiquote() {
    let result = desugar("`(a ,b ,@c)");

    println!("Quasiquote result: {:?}", result);
    assert!(
        result.is_ok(),
        "Should desugar quasiquote: {:?}",
        result.err()
    );
}

#[test]
fn test_desugar_application() {
    let result = desugar("(+ 1 2 3)");

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

/// Test that desugarer returns Literal(Unspecified) for define-syntax
/// (macro is compiled immediately and installed in environment)
#[test]
fn test_desugarer_returns_unspecified_for_define_syntax() {
    // Seeded, not bare: since stage 2 the desugarer recognizes `define-syntax`
    // only through a binding, so an empty environment would compile this to a
    // call of an unbound variable. `core_syntax_list.rs` pins that on purpose;
    // here it would just be noise.
    let (env, heap) = seeded();
    let desugarer = Desugarer::with_env(env);
    let result = desugar_with(
        &desugarer,
        &heap,
        "(define-syntax foo (syntax-rules () ((foo x) x)))",
    )
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
    // Seeded, not bare: since stage 2 the desugarer recognizes `define-syntax`
    // only through a binding, so an empty environment would compile this to a
    // call of an unbound variable. `core_syntax_list.rs` pins that on purpose;
    // here it would just be noise.
    let (env, heap) = seeded();
    let desugarer = Desugarer::with_env(env.clone());
    let _ = desugar_with(
        &desugarer,
        &heap,
        "(define-syntax foo (syntax-rules () ((foo x) x)))",
    )
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
