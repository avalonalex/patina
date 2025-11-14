//! Test helpers for R7RS compliance testing
//!
//! Provides utilities for writing concise tests comparing Patina output
//! against expected values.

#![allow(dead_code)]

use patina_interpreter::{TreeWalkInterpreter, Value};

/// Assert that evaluating a Scheme expression produces the expected result
pub fn assert_eval_to(expr: &str, expected: &str) {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp
        .eval_str(expr)
        .unwrap_or_else(|e| panic!("Failed to evaluate '{}': {}", expr, e));

    let result_str = format!("{}", result);

    assert_eq!(
        result_str, expected,
        "\nExpression: {}\nExpected: {}\nGot: {}",
        expr, expected, result_str
    );
}

/// Assert that evaluating a Scheme expression produces an error
pub fn assert_eval_error(expr: &str) {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str(expr);

    assert!(
        result.is_err(),
        "Expected error for '{}', but got: {:?}",
        expr,
        result.unwrap()
    );
}

/// Assert that evaluating a Scheme expression produces a specific value type
pub fn assert_eval_type(expr: &str, type_check: impl Fn(&Value) -> bool, type_name: &str) {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp
        .eval_str(expr)
        .unwrap_or_else(|e| panic!("Failed to evaluate '{}': {}", expr, e));

    assert!(
        type_check(&result),
        "Expected {} for '{}', but got: {}",
        type_name,
        expr,
        result
    );
}

/// Evaluate multiple expressions in sequence, return last result
pub fn eval_program(code: &str) -> String {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("Failed to evaluate program: {}", e));
    format!("{}", result)
}

/// Assert that a multi-expression program produces expected result
pub fn assert_program_eval_to(code: &str, expected: &str) {
    let result = eval_program(code);
    assert_eq!(
        result, expected,
        "\nProgram:\n{}\nExpected: {}\nGot: {}",
        code, expected, result
    );
}
