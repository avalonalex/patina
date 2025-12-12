//! Integer division operations
//!
//! This module implements integer division operations:
//! - quotient, remainder, modulo
//! - floor/, floor-quotient, floor-remainder
//! - truncate/, truncate-quotient, truncate-remainder

use super::helpers::numeric_err;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;

/// (quotient n1 n2) - Integer quotient
pub(super) fn quotient(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "quotient")?;
    args[0]
        .numeric_quotient(&args[1])
        .map_err(|e| numeric_err(e, "quotient"))
}

/// (remainder n1 n2) - Integer remainder
pub(super) fn remainder(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "remainder")?;
    args[0]
        .numeric_remainder(&args[1])
        .map_err(|e| numeric_err(e, "remainder"))
}

/// (modulo n1 n2) - Integer modulo
pub(super) fn modulo(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "modulo")?;
    args[0]
        .numeric_modulo(&args[1])
        .map_err(|e| numeric_err(e, "modulo"))
}

/// (floor/ n1 n2) -> quotient remainder
/// Returns two values: floor-quotient and floor-remainder
pub(super) fn floor_div(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor/")?;
    let q = args[0]
        .numeric_floor_quotient(&args[1])
        .map_err(|e| numeric_err(e, "floor/"))?;
    let r = args[0]
        .numeric_floor_remainder(&args[1])
        .map_err(|e| numeric_err(e, "floor/"))?;
    Ok(Value::Values(vec![q, r]))
}

/// (floor-quotient n1 n2) -> quotient
/// Returns floor(n1/n2)
pub(super) fn floor_quotient(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor-quotient")?;
    args[0]
        .numeric_floor_quotient(&args[1])
        .map_err(|e| numeric_err(e, "floor-quotient"))
}

/// (floor-remainder n1 n2) -> remainder
/// Returns n1 - n2 * floor(n1/n2)
pub(super) fn floor_remainder(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor-remainder")?;
    args[0]
        .numeric_floor_remainder(&args[1])
        .map_err(|e| numeric_err(e, "floor-remainder"))
}

/// (truncate/ n1 n2) -> quotient remainder
/// Returns two values: truncate-quotient and truncate-remainder
pub(super) fn truncate_div(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate/")?;
    let q = args[0]
        .numeric_truncate_quotient(&args[1])
        .map_err(|e| numeric_err(e, "truncate/"))?;
    let r = args[0]
        .numeric_truncate_remainder(&args[1])
        .map_err(|e| numeric_err(e, "truncate/"))?;
    Ok(Value::Values(vec![q, r]))
}

/// (truncate-quotient n1 n2) -> quotient
/// Returns truncate(n1/n2)
pub(super) fn truncate_quotient(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate-quotient")?;
    args[0]
        .numeric_truncate_quotient(&args[1])
        .map_err(|e| numeric_err(e, "truncate-quotient"))
}

/// (truncate-remainder n1 n2) -> remainder
/// Returns n1 - n2 * truncate(n1/n2)
pub(super) fn truncate_remainder(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate-remainder")?;
    args[0]
        .numeric_truncate_remainder(&args[1])
        .map_err(|e| numeric_err(e, "truncate-remainder"))
}
