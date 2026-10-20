//! Numeric comparison operations
//!
//! This module implements numeric comparisons:
//! - =  (numeric equality)
//! - <  (less than)
//! - >  (greater than)
//! - <= (less than or equal)
//! - >= (greater than or equal)
//!
//! Each comparison has a fast path for fixnum-only arguments.

use super::helpers::numeric_err;
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

/// (= z1 z2 z3 ...) - Numeric equality
/// Returns #t if all arguments are numerically equal.
///
/// Fast path: If all arguments are fixnums, compares directly.
pub(super) fn numeric_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    // Fast path for fixnums
    if args.iter().all(|a| a.is_fixnum()) {
        let first = args[0].as_fixnum_unchecked();
        for arg in &args[1..] {
            if arg.as_fixnum_unchecked() != first {
                return Ok(Value::FALSE);
            }
        }
        return Ok(Value::TRUE);
    }

    // Slow path

    for i in 0..args.len() - 1 {
        let equal = cx
            .numeric_eq_cmp(args[i], args[i + 1])
            .map_err(|e| numeric_err(e, "="))?;
        if !equal {
            return Ok(Value::FALSE);
        }
    }

    Ok(Value::TRUE)
}

/// (< x1 x2 x3 ...) - Less than
/// Returns #t if arguments are monotonically increasing.
///
/// Fast path: If all arguments are fixnums, compares directly.
pub(super) fn less_than<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    // Fast path for fixnums
    if args.iter().all(|a| a.is_fixnum()) {
        for i in 0..args.len() - 1 {
            if args[i].as_fixnum_unchecked() >= args[i + 1].as_fixnum_unchecked() {
                return Ok(Value::FALSE);
            }
        }
        return Ok(Value::TRUE);
    }

    // Slow path

    for i in 0..args.len() - 1 {
        let lt = cx
            .numeric_lt(args[i], args[i + 1])
            .map_err(|e| numeric_err(e, "<"))?;
        if !lt {
            return Ok(Value::FALSE);
        }
    }

    Ok(Value::TRUE)
}

/// (> x1 x2 x3 ...) - Greater than
/// Returns #t if arguments are monotonically decreasing.
///
/// Fast path: If all arguments are fixnums, compares directly.
pub(super) fn greater_than<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    // Fast path for fixnums
    if args.iter().all(|a| a.is_fixnum()) {
        for i in 0..args.len() - 1 {
            if args[i].as_fixnum_unchecked() <= args[i + 1].as_fixnum_unchecked() {
                return Ok(Value::FALSE);
            }
        }
        return Ok(Value::TRUE);
    }

    // Slow path

    for i in 0..args.len() - 1 {
        let gt = cx
            .numeric_gt(args[i], args[i + 1])
            .map_err(|e| numeric_err(e, ">"))?;
        if !gt {
            return Ok(Value::FALSE);
        }
    }

    Ok(Value::TRUE)
}

/// (<= x1 x2 x3 ...) - Less than or equal
/// Returns #t if arguments are monotonically non-decreasing.
///
/// Fast path: If all arguments are fixnums, compares directly.
pub(super) fn less_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    // Fast path for fixnums
    if args.iter().all(|a| a.is_fixnum()) {
        for i in 0..args.len() - 1 {
            if args[i].as_fixnum_unchecked() > args[i + 1].as_fixnum_unchecked() {
                return Ok(Value::FALSE);
            }
        }
        return Ok(Value::TRUE);
    }

    // Slow path

    for i in 0..args.len() - 1 {
        let le = cx
            .numeric_le(args[i], args[i + 1])
            .map_err(|e| numeric_err(e, "<="))?;
        if !le {
            return Ok(Value::FALSE);
        }
    }

    Ok(Value::TRUE)
}

/// (>= x1 x2 x3 ...) - Greater than or equal
/// Returns #t if arguments are monotonically non-increasing.
///
/// Fast path: If all arguments are fixnums, compares directly.
pub(super) fn greater_equal<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    // Fast path for fixnums
    if args.iter().all(|a| a.is_fixnum()) {
        for i in 0..args.len() - 1 {
            if args[i].as_fixnum_unchecked() < args[i + 1].as_fixnum_unchecked() {
                return Ok(Value::FALSE);
            }
        }
        return Ok(Value::TRUE);
    }

    // Slow path

    for i in 0..args.len() - 1 {
        let ge = cx
            .numeric_ge(args[i], args[i + 1])
            .map_err(|e| numeric_err(e, ">="))?;
        if !ge {
            return Ok(Value::FALSE);
        }
    }

    Ok(Value::TRUE)
}
