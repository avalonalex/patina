//! Basic arithmetic operations
//!
//! This module implements the four basic arithmetic operations:
//! - Addition (+)
//! - Subtraction (-)
//! - Multiplication (*)
//! - Division (/)
//!
//! All operations use Heap arithmetic methods which have built-in fixnum fast paths
//! and handle overflow with automatic BigInt promotion.

use super::helpers::numeric_err;
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

/// (+) or (+ z1 z2 ...) - Addition
/// Returns the sum of its arguments. With no arguments, returns 0.
///
/// Uses Heap::numeric_add which has built-in fixnum fast path and BigInt overflow handling.
pub(super) fn add<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() {
        return Ok(Value::fixnum(0));
    }

    let mut acc = args[0];
    for arg in &args[1..] {
        acc = cx.numeric_add(acc, *arg).map_err(|e| numeric_err(e, "+"))?;
    }

    Ok(acc)
}

/// (-) or (- z1 z2 ...) - Subtraction
/// With one argument, returns its negation.
/// With multiple arguments, subtracts subsequent from the first.
///
/// Uses Heap::numeric_sub/numeric_neg which have built-in fixnum fast paths.
pub(super) fn subtract<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    if args.len() == 1 {
        // Negation: (- x) = -x
        let result = cx.numeric_neg(args[0]).map_err(|e| numeric_err(e, "-"))?;
        return Ok(result);
    }

    let mut acc = args[0];
    for arg in &args[1..] {
        acc = cx.numeric_sub(acc, *arg).map_err(|e| numeric_err(e, "-"))?;
    }

    Ok(acc)
}

/// (*) or (* z1 z2 ...) - Multiplication
/// Returns the product of its arguments. With no arguments, returns 1.
///
/// Uses Heap::numeric_mul which has built-in fixnum fast path and BigInt overflow handling.
pub(super) fn multiply<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() {
        return Ok(Value::fixnum(1));
    }

    let mut acc = args[0];
    for arg in &args[1..] {
        acc = cx.numeric_mul(acc, *arg).map_err(|e| numeric_err(e, "*"))?;
    }

    Ok(acc)
}

/// (/) or (/ z1 z2 ...) - Division
/// With one argument, returns its reciprocal.
/// With multiple arguments, divides the first by subsequent arguments.
///
/// Uses Heap::numeric_div. Note: Division typically produces rationals.
pub(super) fn divide<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    if args.len() == 1 {
        // (/ x) = 1/x
        let one = Value::fixnum(1);
        let result = cx
            .numeric_div(one, args[0])
            .map_err(|e| numeric_err(e, "/"))?;
        return Ok(result);
    }

    let mut acc = args[0];
    for arg in &args[1..] {
        acc = cx.numeric_div(acc, *arg).map_err(|e| numeric_err(e, "/"))?;
    }

    Ok(acc)
}
