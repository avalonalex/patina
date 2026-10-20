//! Transcendental operations
//!
//! This module implements:
//! - Square root and exponentiation: sqrt, square, expt
//! - Trigonometric: sin, cos, tan, asin, acos, atan
//! - Exponential/logarithmic: exp, log
//!
//! All operations use Heap methods on TaggedValues.

use super::helpers::numeric_err;
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

// ========== Square Root and Exponentiation ==========

/// (sqrt x) - Square root
/// Preserves exact roots; other results use floating-point arithmetic.
/// R7RS branch cut: principal square root is always in right half-plane (re >= 0)
pub(super) fn sqrt<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_sqrt(args[0]).map_err(|e| numeric_err(e, "sqrt"))
}

/// (square x) - Square of x
pub(super) fn square<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_square(args[0])
        .map_err(|e| numeric_err(e, "square"))
}

/// (expt base power) - Exponentiation
pub(super) fn expt<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_expt(args[0], args[1])
        .map_err(|e| numeric_err(e, "expt"))
}

// ========== Trigonometric Functions ==========

/// (sin x) - Sine
/// Supports complex numbers: sin(z) = (e^(iz) - e^(-iz)) / 2i
pub(super) fn sin<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_sin(args[0]).map_err(|e| numeric_err(e, "sin"))
}

/// (cos x) - Cosine
/// Supports complex numbers: cos(z) = (e^(iz) + e^(-iz)) / 2
pub(super) fn cos<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_cos(args[0]).map_err(|e| numeric_err(e, "cos"))
}

/// (tan x) - Tangent
/// Supports complex numbers: tan(z) = sin(z) / cos(z)
pub(super) fn tan<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_tan(args[0]).map_err(|e| numeric_err(e, "tan"))
}

/// (asin x) - Arc sine
/// Supports complex numbers with branch cuts
pub(super) fn asin<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_asin(args[0]).map_err(|e| numeric_err(e, "asin"))
}

/// (acos x) - Arc cosine
/// Supports complex numbers with branch cuts
pub(super) fn acos<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_acos(args[0]).map_err(|e| numeric_err(e, "acos"))
}

/// (atan x [y]) - Arc tangent (one or two arguments)
/// One argument: supports complex numbers with branch cuts
/// Two arguments: real-only atan2(y, x)
pub(super) fn atan<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }
    if args.len() == 1 {
        cx.numeric_atan(args[0]).map_err(|e| numeric_err(e, "atan"))
    } else {
        cx.numeric_atan2(args[0], args[1])
            .map_err(|e| numeric_err(e, "atan"))
    }
}

// ========== Exponential and Logarithmic Functions ==========

/// (exp x) - e^x
/// Supports complex numbers: e^(a+bi) = e^a * (cos(b) + i*sin(b))
pub(super) fn exp<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    cx.numeric_exp(args[0]).map_err(|e| numeric_err(e, "exp"))
}

/// (log x [base]) - Natural log or log with base
/// Supports complex numbers with branch cut on negative real axis
pub(super) fn log<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }
    if args.len() == 1 {
        cx.numeric_log(args[0]).map_err(|e| numeric_err(e, "log"))
    } else {
        cx.numeric_log_base(args[0], args[1])
            .map_err(|e| numeric_err(e, "log"))
    }
}
