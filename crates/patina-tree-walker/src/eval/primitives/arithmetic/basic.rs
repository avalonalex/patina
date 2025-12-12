//! Basic arithmetic operations
//!
//! This module implements the four basic arithmetic operations:
//! - Addition (+)
//! - Subtraction (-)
//! - Multiplication (*)
//! - Division (/)

use super::helpers::numeric_err;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;

/// (+) or (+ z1 z2 ...) - Addition
/// Returns the sum of its arguments. With no arguments, returns 0.
pub(super) fn add(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Ok(Value::Integer(0));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_add(arg).map_err(|e| numeric_err(e, "+"))?;
    }

    Ok(result)
}

/// (-) or (- z1 z2 ...) - Subtraction
/// With one argument, returns its negation.
/// With multiple arguments, subtracts subsequent from the first.
pub(super) fn subtract(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    if args.len() == 1 {
        return args[0].numeric_neg().map_err(|e| numeric_err(e, "-"));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_sub(arg).map_err(|e| numeric_err(e, "-"))?;
    }

    Ok(result)
}

/// (*) or (* z1 z2 ...) - Multiplication
/// Returns the product of its arguments. With no arguments, returns 1.
pub(super) fn multiply(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Ok(Value::Integer(1));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_mul(arg).map_err(|e| numeric_err(e, "*"))?;
    }

    Ok(result)
}

/// (/) or (/ z1 z2 ...) - Division
/// With one argument, returns its reciprocal.
/// With multiple arguments, divides the first by subsequent arguments.
pub(super) fn divide(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    if args.len() == 1 {
        // (/ x) = 1/x
        return Value::Integer(1)
            .numeric_div(&args[0])
            .map_err(|e| numeric_err(e, "/"));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_div(arg).map_err(|e| numeric_err(e, "/"))?;
    }

    Ok(result)
}
