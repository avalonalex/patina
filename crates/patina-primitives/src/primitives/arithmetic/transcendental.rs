//! Transcendental operations
//!
//! This module implements:
//! - Square root and exponentiation: sqrt, square, expt
//! - Trigonometric: sin, cos, tan, asin, acos, atan
//! - Exponential/logarithmic: exp, log
//!
//! All operations use Heap methods which delegate to Value methods.

use super::helpers::numeric_err;
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

// ========== Square Root and Exponentiation ==========

/// (sqrt x) - Square root
/// Uses Complex64 to handle all cases uniformly, including negative reals
/// R7RS branch cut: principal square root is always in right half-plane (re >= 0)
pub(super) fn sqrt(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_sqrt(args[0])
        .map_err(|e| numeric_err(e, "sqrt"))
}

/// (square x) - Square of x
pub(super) fn square(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_square(args[0])
        .map_err(|e| numeric_err(e, "square"))
}

/// (expt base power) - Exponentiation
pub(super) fn expt(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_expt(args[0], args[1])
        .map_err(|e| numeric_err(e, "expt"))
}

// ========== Trigonometric Functions ==========

/// (sin x) - Sine
/// Supports complex numbers: sin(z) = (e^(iz) - e^(-iz)) / 2i
pub(super) fn sin(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_sin(args[0])
        .map_err(|e| numeric_err(e, "sin"))
}

/// (cos x) - Cosine
/// Supports complex numbers: cos(z) = (e^(iz) + e^(-iz)) / 2
pub(super) fn cos(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_cos(args[0])
        .map_err(|e| numeric_err(e, "cos"))
}

/// (tan x) - Tangent
/// Supports complex numbers: tan(z) = sin(z) / cos(z)
pub(super) fn tan(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_tan(args[0])
        .map_err(|e| numeric_err(e, "tan"))
}

/// (asin x) - Arc sine
/// Supports complex numbers with branch cuts
pub(super) fn asin(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_asin(args[0])
        .map_err(|e| numeric_err(e, "asin"))
}

/// (acos x) - Arc cosine
/// Supports complex numbers with branch cuts
pub(super) fn acos(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_acos(args[0])
        .map_err(|e| numeric_err(e, "acos"))
}

/// (atan x [y]) - Arc tangent (one or two arguments)
/// One argument: supports complex numbers with branch cuts
/// Two arguments: real-only atan2(y, x)
pub(super) fn atan(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }
    if args.len() == 1 {
        heap.borrow_mut()
            .numeric_atan(args[0])
            .map_err(|e| numeric_err(e, "atan"))
    } else {
        heap.borrow_mut()
            .numeric_atan2(args[0], args[1])
            .map_err(|e| numeric_err(e, "atan"))
    }
}

// ========== Exponential and Logarithmic Functions ==========

/// (exp x) - e^x
/// Supports complex numbers: e^(a+bi) = e^a * (cos(b) + i*sin(b))
pub(super) fn exp(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_exp(args[0])
        .map_err(|e| numeric_err(e, "exp"))
}

/// (log x [base]) - Natural log or log with base
/// Supports complex numbers with branch cut on negative real axis
pub(super) fn log(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }
    if args.len() == 1 {
        heap.borrow_mut()
            .numeric_log(args[0])
            .map_err(|e| numeric_err(e, "log"))
    } else {
        heap.borrow_mut()
            .numeric_log_base(args[0], args[1])
            .map_err(|e| numeric_err(e, "log"))
    }
}
