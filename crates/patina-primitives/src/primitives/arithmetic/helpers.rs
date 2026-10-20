//! Helper functions for arithmetic operations
//!
//! This module provides error conversion and utility functions
//! used throughout the arithmetic modules.

use num_bigint::BigInt;
use num_traits::ToPrimitive;
use patina_core::numeric::NumericError;
use patina_core::{Cx, TaggedValue, Value};
use patina_runtime::EvalError;

/// Convert a NumericError to an EvalError with context
pub(super) fn numeric_err(e: NumericError, op: &str) -> EvalError {
    match e {
        NumericError::NotANumber { got } => {
            EvalError::TypeError(format!("{}: expected number, got {}", op, got))
        }
        NumericError::NotReal { got } => {
            EvalError::TypeError(format!("{}: expected real number, got {}", op, got))
        }
        NumericError::NotInteger { got } => {
            EvalError::TypeError(format!("{}: expected integer, got {}", op, got))
        }
        NumericError::NotExact { got } => {
            EvalError::TypeError(format!("{}: expected exact number, got {}", op, got))
        }
        NumericError::DivisionByZero => EvalError::DivisionByZero,
        NumericError::Undefined { operation, reason } => {
            EvalError::TypeError(format!("{}: {} - {}", op, operation, reason))
        }
        NumericError::NoExactRepresentation { value } => {
            EvalError::TypeError(format!("{}: no exact representation for {}", op, value))
        }
    }
}

/// Helper to convert BigRational to the appropriate value type
/// Simplifies to fixnum or BigInteger if denominator is 1
pub(crate) fn rational_to_value<'gc>(cx: &mut Cx<'gc>, r: num_rational::BigRational) -> Value<'gc> {
    if r.denom() == &BigInt::from(1) {
        // Denominator is 1, simplify to integer
        let numer = r.numer();
        if let Some(n) = numer.to_i64().filter(|n| TaggedValue::fits_fixnum(*n)) {
            Value::fixnum(n)
        } else {
            cx.alloc_bigint(numer.clone())
        }
    } else {
        cx.alloc_rational(r)
    }
}
