//! Numeric comparison operations
//!
//! This module implements numeric comparisons:
//! - =  (numeric equality)
//! - <  (less than)
//! - >  (greater than)
//! - <= (less than or equal)
//! - >= (greater than or equal)

use super::helpers::numeric_err;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;

/// (= z1 z2 z3 ...) - Numeric equality
/// Returns #t if all arguments are numerically equal.
pub(super) fn numeric_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "=")?;

    for i in 0..args.len() - 1 {
        let equal = args[i]
            .numeric_eq(&args[i + 1])
            .map_err(|e| numeric_err(e, "="))?;
        if !equal {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

/// (< x1 x2 x3 ...) - Less than
/// Returns #t if arguments are monotonically increasing.
pub(super) fn less_than(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "<")?;

    for i in 0..args.len() - 1 {
        let lt = args[i]
            .numeric_lt(&args[i + 1])
            .map_err(|e| numeric_err(e, "<"))?;
        if !lt {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

/// (> x1 x2 x3 ...) - Greater than
/// Returns #t if arguments are monotonically decreasing.
pub(super) fn greater_than(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, ">")?;

    for i in 0..args.len() - 1 {
        let gt = args[i]
            .numeric_gt(&args[i + 1])
            .map_err(|e| numeric_err(e, ">"))?;
        if !gt {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

/// (<= x1 x2 x3 ...) - Less than or equal
/// Returns #t if arguments are monotonically non-decreasing.
pub(super) fn less_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "<=")?;

    for i in 0..args.len() - 1 {
        let le = args[i]
            .numeric_le(&args[i + 1])
            .map_err(|e| numeric_err(e, "<="))?;
        if !le {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

/// (>= x1 x2 x3 ...) - Greater than or equal
/// Returns #t if arguments are monotonically non-increasing.
pub(super) fn greater_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, ">=")?;

    for i in 0..args.len() - 1 {
        let ge = args[i]
            .numeric_ge(&args[i + 1])
            .map_err(|e| numeric_err(e, ">="))?;
        if !ge {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}
