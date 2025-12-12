//! Complex number operations
//!
//! This module implements:
//! - real-part, imag-part - Complex number accessors
//! - magnitude, angle - Polar form accessors
//! - make-rectangular, make-polar - Complex number constructors

use super::helpers::numeric_err;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_core::numeric;
use patina_runtime::value::Value;

/// (real-part z) - Real part of complex number
/// IMPORTANT: Returns the real part preserving its exactness
pub(super) fn real_part(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "real-part")?;
    args[0].real_part().map_err(|e| numeric_err(e, "real-part"))
}

/// (imag-part z) - Imaginary part of complex number
/// IMPORTANT: Returns the imaginary part preserving its exactness
pub(super) fn imag_part(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "imag-part")?;
    args[0].imag_part().map_err(|e| numeric_err(e, "imag-part"))
}

/// (magnitude z) - Magnitude of complex number
pub(super) fn magnitude(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "magnitude")?;
    args[0].magnitude().map_err(|e| numeric_err(e, "magnitude"))
}

/// (angle z) - Angle of complex number in radians
pub(super) fn angle(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "angle")?;
    args[0].angle().map_err(|e| numeric_err(e, "angle"))
}

/// (make-rectangular real imag) - Construct complex number from real and imaginary parts
/// Preserves exactness of the arguments
pub(super) fn make_rectangular(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "make-rectangular")?;
    numeric::make_rectangular(&args[0], &args[1]).map_err(|e| numeric_err(e, "make-rectangular"))
}

/// (make-polar magnitude angle) - Construct complex number from polar coordinates
/// Always produces inexact results since trig functions are inexact
pub(super) fn make_polar(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "make-polar")?;
    numeric::make_polar(&args[0], &args[1]).map_err(|e| numeric_err(e, "make-polar"))
}
