//! Rounding and utility operations
//!
//! This module implements:
//! - Rounding: floor, ceiling, truncate, round
//! - Utilities: abs, max, min

use super::helpers::numeric_err;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;

/// (floor x) - Rounds x toward negative infinity
pub(super) fn floor(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "floor")?;
    args[0].numeric_floor().map_err(|e| numeric_err(e, "floor"))
}

/// (ceiling x) - Rounds x toward positive infinity
pub(super) fn ceiling(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "ceiling")?;
    args[0]
        .numeric_ceiling()
        .map_err(|e| numeric_err(e, "ceiling"))
}

/// (truncate x) - Rounds x toward zero
pub(super) fn truncate(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "truncate")?;
    args[0]
        .numeric_truncate()
        .map_err(|e| numeric_err(e, "truncate"))
}

/// (round x) - Rounds x to nearest integer (banker's rounding: ties round to even)
pub(super) fn round(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "round")?;
    args[0].numeric_round().map_err(|e| numeric_err(e, "round"))
}

/// (abs x) - Absolute value
pub(super) fn abs(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "abs")?;
    args[0].numeric_abs().map_err(|e| numeric_err(e, "abs"))
}

/// (max x1 x2 ...) - Maximum of arguments
pub(super) fn max(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "max")?;

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_max(arg).map_err(|e| numeric_err(e, "max"))?;
    }
    Ok(result)
}

/// (min x1 x2 ...) - Minimum of arguments
pub(super) fn min(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "min")?;

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_min(arg).map_err(|e| numeric_err(e, "min"))?;
    }
    Ok(result)
}
