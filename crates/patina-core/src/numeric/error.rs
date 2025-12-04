//! Numeric error types

use std::fmt;

/// Errors that can occur during numeric operations
#[derive(Debug, Clone, PartialEq)]
pub enum NumericError {
    /// Operation requires a number but got something else
    NotANumber { got: String },

    /// Operation requires a real number (not complex)
    NotReal { got: String },

    /// Operation requires an integer
    NotInteger { got: String },

    /// Operation requires an exact number
    NotExact { got: String },

    /// Division by zero (exact zero)
    DivisionByZero,

    /// Result would be undefined (e.g., log of negative without complex support)
    Undefined {
        operation: &'static str,
        reason: &'static str,
    },

    /// No exact representation exists (e.g., exact of NaN or infinity)
    NoExactRepresentation { value: String },
}

impl fmt::Display for NumericError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumericError::NotANumber { got } => {
                write!(f, "expected number, got {}", got)
            }
            NumericError::NotReal { got } => {
                write!(f, "expected real number, got {}", got)
            }
            NumericError::NotInteger { got } => {
                write!(f, "expected integer, got {}", got)
            }
            NumericError::NotExact { got } => {
                write!(f, "expected exact number, got {}", got)
            }
            NumericError::DivisionByZero => {
                write!(f, "division by zero")
            }
            NumericError::Undefined { operation, reason } => {
                write!(f, "{}: {}", operation, reason)
            }
            NumericError::NoExactRepresentation { value } => {
                write!(f, "no exact representation for {}", value)
            }
        }
    }
}

impl std::error::Error for NumericError {}
