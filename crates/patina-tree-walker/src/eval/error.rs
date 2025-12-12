//! Error types for the Scheme evaluator
//!
//! This module defines all error types that can occur during evaluation of Scheme expressions.

use patina_frontend::FrontendError;
use thiserror::Error;

/// The kind of Scheme exception, matching ExceptionKind in patina-core
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemeExceptionKind {
    Error,
    FileError,
    ReadError,
    Custom(String),
}

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

    /// A continuation was invoked that escapes the current evaluation context.
    /// The actual continuation data is stored in thread-local storage
    /// (see cps_eval::PENDING_CONTINUATION_ESCAPE).
    #[error("Continuation escape")]
    ContinuationEscape,

    /// A Scheme-level exception was raised (R7RS Section 6.11)
    /// We store only the message and kind (not the full Value) to satisfy Send+Sync.
    /// The irritants are serialized to a string for the error message.
    /// This can be caught by `guard` or `with-exception-handler`.
    #[error("Scheme exception ({kind:?}): {message}")]
    SchemeException {
        kind: SchemeExceptionKind,
        message: String,
        irritants_display: String, // Serialized display of irritants
    },
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
