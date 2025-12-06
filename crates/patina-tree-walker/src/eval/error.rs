//! Error types for the Scheme evaluator
//!
//! This module defines all error types that can occur during evaluation of Scheme expressions.

use patina_frontend::FrontendError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EvalError {
    #[error("Undefined variable: {0}")]
    UndefinedVariable(String),

    #[error("Not a procedure: {0}")]
    NotAProcedure(String),

    #[error("Wrong number of arguments: expected {expected}, got {actual}")]
    WrongArity { expected: String, actual: usize },

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    #[error("Type error: {0}")]
    TypeError(String),

    #[error("Division by zero")]
    DivisionByZero,

    #[error("Index out of bounds: {0}")]
    IndexOutOfBounds(String),

    #[error("I/O error: {0}")]
    IOError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

// Convert FrontendError to EvalError
impl From<FrontendError> for EvalError {
    fn from(err: FrontendError) -> Self {
        match err {
            FrontendError::InvalidSyntax(msg) => EvalError::InvalidSyntax(msg),
            FrontendError::MacroError(msg) => EvalError::InvalidSyntax(msg),
            FrontendError::TypeError(msg) => EvalError::TypeError(msg),
            _ => EvalError::InternalError(format!("Frontend error: {}", err)),
        }
    }
}
