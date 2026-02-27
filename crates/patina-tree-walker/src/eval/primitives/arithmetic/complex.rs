//! Complex number operations
//!
//! This module implements:
//! - real-part, imag-part - Complex number accessors
//! - magnitude, angle - Polar form accessors
//! - make-rectangular, make-polar - Complex number constructors
//!
//! All operations use Heap methods which have built-in fixnum fast paths.

use super::helpers::numeric_err;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use patina_core::TaggedValue;

/// (real-part z) - Real part of complex number
/// Uses Heap::real_part with built-in fixnum fast path.
pub(super) fn real_part(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    heap.borrow_mut()
        .real_part(args[0])
        .map_err(|e| numeric_err(e, "real-part"))
}

/// (imag-part z) - Imaginary part of complex number
/// Uses Heap::imag_part with built-in fixnum fast path.
pub(super) fn imag_part(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    heap.borrow_mut()
        .imag_part(args[0])
        .map_err(|e| numeric_err(e, "imag-part"))
}

/// (magnitude z) - Magnitude of complex number
/// Uses Heap::magnitude with built-in fixnum fast path.
pub(super) fn magnitude(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    heap.borrow_mut()
        .magnitude(args[0])
        .map_err(|e| numeric_err(e, "magnitude"))
}

/// (angle z) - Angle of complex number in radians
/// Uses Heap::angle with built-in fixnum fast path.
pub(super) fn angle(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    heap.borrow_mut()
        .angle(args[0])
        .map_err(|e| numeric_err(e, "angle"))
}

/// (make-rectangular real imag) - Construct complex number from real and imaginary parts
/// Uses Heap::make_rectangular with built-in fast path for zero imaginary.
pub(super) fn make_rectangular(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    heap.borrow_mut()
        .make_rectangular(args[0], args[1])
        .map_err(|e| numeric_err(e, "make-rectangular"))
}

/// (make-polar magnitude angle) - Construct complex number from polar coordinates
/// Uses Heap::make_polar. Always produces inexact results since trig functions are inexact.
pub(super) fn make_polar(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    heap.borrow_mut()
        .make_polar(args[0], args[1])
        .map_err(|e| numeric_err(e, "make-polar"))
}
