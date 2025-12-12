//! Transcendental operations
//!
//! This module implements:
//! - Square root and exponentiation: sqrt, square, expt
//! - Trigonometric: sin, cos, tan, asin, acos, atan
//! - Exponential/logarithmic: exp, log

use super::helpers::numeric_err;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_runtime::value::Value;

// ========== Square Root and Exponentiation ==========

/// (sqrt x) - Square root
/// Uses Complex64 to handle all cases uniformly, including negative reals
/// R7RS branch cut: principal square root is always in right half-plane (re >= 0)
pub(super) fn sqrt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "sqrt")?;
    args[0].numeric_sqrt().map_err(|e| numeric_err(e, "sqrt"))
}

/// (square x) - Square of x
pub(super) fn square(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "square")?;
    args[0]
        .numeric_square()
        .map_err(|e| numeric_err(e, "square"))
}

/// (expt base power) - Exponentiation
pub(super) fn expt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "expt")?;
    args[0]
        .numeric_expt(&args[1])
        .map_err(|e| numeric_err(e, "expt"))
}

// ========== Trigonometric Functions ==========

/// (sin x) - Sine
/// Supports complex numbers: sin(z) = (e^(iz) - e^(-iz)) / 2i
pub(super) fn sin(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "sin")?;
    args[0].numeric_sin().map_err(|e| numeric_err(e, "sin"))
}

/// (cos x) - Cosine
/// Supports complex numbers: cos(z) = (e^(iz) + e^(-iz)) / 2
pub(super) fn cos(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "cos")?;
    args[0].numeric_cos().map_err(|e| numeric_err(e, "cos"))
}

/// (tan x) - Tangent
/// Supports complex numbers: tan(z) = sin(z) / cos(z)
pub(super) fn tan(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "tan")?;
    args[0].numeric_tan().map_err(|e| numeric_err(e, "tan"))
}

/// (asin x) - Arc sine
/// Supports complex numbers with branch cuts
pub(super) fn asin(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "asin")?;
    args[0].numeric_asin().map_err(|e| numeric_err(e, "asin"))
}

/// (acos x) - Arc cosine
/// Supports complex numbers with branch cuts
pub(super) fn acos(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "acos")?;
    args[0].numeric_acos().map_err(|e| numeric_err(e, "acos"))
}

/// (atan x [y]) - Arc tangent (one or two arguments)
/// One argument: supports complex numbers with branch cuts
/// Two arguments: real-only atan2(y, x)
pub(super) fn atan(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_range(&args, 1, 2, "atan")?;

    if args.len() == 1 {
        args[0].numeric_atan().map_err(|e| numeric_err(e, "atan"))
    } else {
        args[0]
            .numeric_atan2(&args[1])
            .map_err(|e| numeric_err(e, "atan"))
    }
}

// ========== Exponential and Logarithmic Functions ==========

/// (exp x) - e^x
/// Supports complex numbers: e^(a+bi) = e^a * (cos(b) + i*sin(b))
pub(super) fn exp(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exp")?;
    args[0].numeric_exp().map_err(|e| numeric_err(e, "exp"))
}

/// (log x [base]) - Natural log or log with base
/// Supports complex numbers with branch cut on negative real axis
pub(super) fn log(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_range(&args, 1, 2, "log")?;

    if args.len() == 1 {
        args[0].numeric_log().map_err(|e| numeric_err(e, "log"))
    } else {
        args[0]
            .numeric_log_base(&args[1])
            .map_err(|e| numeric_err(e, "log"))
    }
}
