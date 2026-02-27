//! Helper functions for arithmetic operations
//!
//! This module provides error conversion and utility functions
//! used throughout the arithmetic modules.

use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use patina_core::TaggedValue;
use patina_core::numeric::NumericError;

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

impl Evaluator {
    /// Helper to convert BigRational to the appropriate TaggedValue type
    /// Simplifies to fixnum or BigInteger if denominator is 1
    pub(in crate::eval) fn rational_to_tagged(&self, r: num_rational::BigRational) -> TaggedValue {
        if r.denom() == &BigInt::from(1) {
            // Denominator is 1, simplify to integer
            let numer = r.numer();
            if let Some(n) = numer.to_i64().filter(|n| TaggedValue::fits_fixnum(*n)) {
                TaggedValue::fixnum(n)
            } else {
                self.global_env
                    .heap()
                    .borrow_mut()
                    .alloc_bigint(numer.clone())
            }
        } else {
            self.global_env.heap().borrow_mut().alloc_rational(r)
        }
    }
}
