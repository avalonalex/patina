//! Integration tests for CoreExpr pipeline
//!
//! These tests verify the full pipeline: parse → macro expand → desugar → eval
//! to understand what's breaking in the CoreExpr integration.

mod common;
use patina_frontend::{Desugarer, Parser};
use patina_tree_walker::{Evaluator, eval::eval_core};

#[test]
fn test_full_pipeline_simple_or() {
    let evaluator = Evaluator::new();
    let code = "(or #f #t)";

    // Parse
    let mut parser = Parser::new(code).unwrap();
    let value = parser.parse().unwrap();
    println!("Parsed: {}", value);

    // Expand macros
    let expanded = evaluator
        .expand_all_macros(&value, &evaluator.global_env)
        .unwrap();
    println!("Expanded: {}", expanded);

    // Desugar
    let desugarer = Desugarer::new();
    let core_expr = desugarer.desugar(&expanded).unwrap();
    println!("Desugared: {:?}", core_expr);

    // Evaluate through CoreExpr
    let result = eval_core(&core_expr, evaluator.global_env.clone(), &evaluator).unwrap();
    println!("Result: {}", result);

    assert_eq!(result.to_string(), "#t");
}

#[test]
fn test_full_pipeline_let() {
    let evaluator = Evaluator::new();
    let code = "(let ((x 42)) x)";

    let mut parser = Parser::new(code).unwrap();
    let value = parser.parse().unwrap();
    println!("\nParsed: {}", value);

    let expanded = evaluator
        .expand_all_macros(&value, &evaluator.global_env)
        .unwrap();
    println!("Expanded: {}", expanded);

    let desugarer = Desugarer::new();
    let core_expr = desugarer.desugar(&expanded).unwrap();
    println!("Desugared: {:?}", core_expr);

    let result = eval_core(&core_expr, evaluator.global_env.clone(), &evaluator).unwrap();
    println!("Result: {}", result);

    assert_eq!(result.to_string(), "42");
}

#[test]
fn test_full_pipeline_nested_macros() {
    let evaluator = Evaluator::new();
    let code = "(let ((x 1)) (or (= x 1) (= x 2)))";

    let mut parser = Parser::new(code).unwrap();
    let value = parser.parse().unwrap();
    println!("\nParsed: {}", value);

    let expanded = evaluator
        .expand_all_macros(&value, &evaluator.global_env)
        .unwrap();
    println!("Expanded: {}", expanded);

    let desugarer = Desugarer::new();
    let core_expr = desugarer.desugar(&expanded).unwrap();
    println!("Desugared: {:?}", core_expr);

    let result = eval_core(&core_expr, evaluator.global_env.clone(), &evaluator).unwrap();
    println!("Result: {}", result);

    assert_eq!(result.to_string(), "#t");
}

#[test]
fn test_full_pipeline_lambda_with_macros() {
    let evaluator = Evaluator::new();
    let code = "(define (f x) (or (= x 1) (= x 2)))";

    let mut parser = Parser::new(code).unwrap();
    let value = parser.parse().unwrap();
    println!("\nParsed: {}", value);

    let expanded = evaluator
        .expand_all_macros(&value, &evaluator.global_env)
        .unwrap();
    println!("Expanded: {}", expanded);

    let desugarer = Desugarer::new();
    let core_expr = desugarer.desugar(&expanded).unwrap();
    println!("Desugared: {:?}", core_expr);

    let result = eval_core(&core_expr, evaluator.global_env.clone(), &evaluator).unwrap();
    println!("Result: {}", result);

    // Now call the function
    let call_code = "(f 1)";
    let mut parser2 = Parser::new(call_code).unwrap();
    let call_value = parser2.parse().unwrap();

    let call_expanded = evaluator
        .expand_all_macros(&call_value, &evaluator.global_env)
        .unwrap();
    let call_core = desugarer.desugar(&call_expanded).unwrap();
    let call_result = eval_core(&call_core, evaluator.global_env.clone(), &evaluator).unwrap();

    println!("Function call result: {}", call_result);
    assert_eq!(call_result.to_string(), "#t");
}

#[test]
fn test_compare_value_vs_core_expr_evaluators() {
    // Compare results from Value evaluator vs CoreExpr evaluator
    let evaluator = Evaluator::new();
    let test_cases = vec![
        "(or #f #t)",
        "(and #t #t)",
        "(let ((x 1)) x)",
        "(cond (#t 'yes))",
        "((lambda (x) (if x 1 2)) #t)",
    ];

    for code in test_cases {
        println!("\n=== Testing: {} ===", code);

        let mut parser = Parser::new(code).unwrap();
        let value = parser.parse().unwrap();

        // Value evaluator path (current working approach)
        let value_result = evaluator
            .eval_in_env(&value, &evaluator.global_env)
            .unwrap();
        println!("Value evaluator: {}", value_result);

        // CoreExpr evaluator path (what we're trying to enable)
        let expanded = evaluator
            .expand_all_macros(&value, &evaluator.global_env)
            .unwrap();
        println!("After macro expansion: {}", expanded);

        let desugarer = Desugarer::new();
        match desugarer.desugar(&expanded) {
            Ok(core_expr) => {
                println!("Desugared: {:?}", core_expr);
                match eval_core(&core_expr, evaluator.global_env.clone(), &evaluator) {
                    Ok(core_result) => {
                        println!("CoreExpr evaluator: {}", core_result);
                        assert_eq!(
                            value_result.to_string(),
                            core_result.to_string(),
                            "Results should match for: {}",
                            code
                        );
                    }
                    Err(e) => {
                        panic!("CoreExpr evaluation failed for {}: {:?}", code, e);
                    }
                }
            }
            Err(e) => {
                panic!("Desugaring failed for {}: {:?}", code, e);
            }
        }
    }
}
