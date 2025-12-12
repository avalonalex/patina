//! Float predicates
//!
//! This module implements:
//! - finite? - Returns #t if x is finite
//! - infinite? - Returns #t if x is infinite
//! - nan? - Returns #t if x is NaN

use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;

/// (finite? x) - Returns #t if x is finite
pub(super) fn finite_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "finite?")?;
    if !args[0].is_number() {
        return Err(EvalError::TypeError(format!(
            "finite? expects a number, got {}",
            args[0].type_name()
        )));
    }
    Ok(Value::Boolean(args[0].is_finite()))
}

/// (infinite? x) - Returns #t if x is infinite
pub(super) fn infinite_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "infinite?")?;
    if !args[0].is_number() {
        return Err(EvalError::TypeError(format!(
            "infinite? expects a number, got {}",
            args[0].type_name()
        )));
    }
    Ok(Value::Boolean(args[0].is_infinite()))
}

/// (nan? x) - Returns #t if x is NaN
pub(super) fn nan_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "nan?")?;
    if !args[0].is_number() {
        return Err(EvalError::TypeError(format!(
            "nan? expects a number, got {}",
            args[0].type_name()
        )));
    }
    Ok(Value::Boolean(args[0].is_nan()))
}
