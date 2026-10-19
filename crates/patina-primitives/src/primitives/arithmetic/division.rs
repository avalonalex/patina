//! Integer division operations
//!
//! This module implements integer division operations:
//! - quotient, remainder, modulo
//! - floor/, floor-quotient, floor-remainder
//! - truncate/, truncate-quotient, truncate-remainder
//!
//! All operations use Heap methods which have built-in fixnum fast paths.

use super::helpers::numeric_err;
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

/// (quotient n1 n2) - Integer quotient
/// Uses Heap::numeric_quotient with built-in fixnum fast path.
pub(super) fn quotient<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "quotient"))
}

/// (remainder n1 n2) - Integer remainder
/// Uses Heap::numeric_remainder with built-in fixnum fast path.
pub(super) fn remainder<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "remainder"))
}

/// (modulo n1 n2) - Integer modulo
/// Uses Heap::numeric_modulo with built-in fixnum fast path.
pub(super) fn modulo<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_modulo(args[0], args[1])
        .map_err(|e| numeric_err(e, "modulo"))
}

/// (floor/ n1 n2) -> quotient remainder
/// Returns two values: floor-quotient and floor-remainder.
/// Uses Heap methods with built-in fixnum fast paths.
pub(super) fn floor_div<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let q = cx
        .numeric_floor_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "floor/"))?;
    let r = cx
        .numeric_floor_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "floor/"))?;

    Ok(cx.alloc_values(vec![q, r]))
}

/// (floor-quotient n1 n2) -> quotient
/// Returns floor(n1/n2).
/// Uses Heap::numeric_floor_quotient with built-in fixnum fast path.
pub(super) fn floor_quotient<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_floor_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "floor-quotient"))
}

/// (floor-remainder n1 n2) -> remainder
/// Returns n1 - n2 * floor(n1/n2).
/// Uses Heap::numeric_floor_remainder with built-in fixnum fast path.
pub(super) fn floor_remainder<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_floor_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "floor-remainder"))
}

/// (truncate/ n1 n2) -> quotient remainder
/// Returns two values: truncate-quotient and truncate-remainder.
/// truncate operations are the same as quotient/remainder (truncation toward zero).
pub(super) fn truncate_div<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    // truncate-quotient == quotient, truncate-remainder == remainder
    let q = cx
        .numeric_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "truncate/"))?;
    let r = cx
        .numeric_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "truncate/"))?;

    Ok(cx.alloc_values(vec![q, r]))
}

/// (truncate-quotient n1 n2) -> quotient
/// Returns truncate(n1/n2). Same as quotient (truncation toward zero).
pub(super) fn truncate_quotient<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "truncate-quotient"))
}

/// (truncate-remainder n1 n2) -> remainder
/// Returns n1 - n2 * truncate(n1/n2). Same as remainder.
pub(super) fn truncate_remainder<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "truncate-remainder"))
}
