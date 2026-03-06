//! Error types for the Scheme evaluator
//!
//! This module defines all error types that can occur during evaluation of Scheme expressions.
//!
//! Moved from patina-tree-walker to patina-runtime so that the patina-primitives crate
//! and future VM backends can share the same error type without depending on tree-walker.
//!
//! ## Error Categories
//!
//! Errors are categorized by whether they can be caught by Scheme exception handlers:
//!
//! - **Catchable**: Can be caught by `guard` or `with-exception-handler`
//!   - `SchemeException`: Wraps Scheme-level exceptions
//!   - All other runtime errors (type, arity, bounds, etc.)
//!
//! - **Non-catchable**: Stay in Rust, cannot be caught in Scheme
//!   - `InternalError`: Interpreter bugs
//!   - `ContinuationEscape`: Control flow mechanism

use patina_core::error::SourceLocation;
use patina_core::{ErrorDetail, ErrorKind, ExceptionKind};
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

    /// A continuation was invoked that escapes the current evaluation context.
    /// The actual continuation data is stored in thread-local storage
    /// (see cps_eval::PENDING_CONTINUATION_ESCAPE).
    #[error("Continuation escape")]
    ContinuationEscape,

    /// A Scheme-level exception was raised (R7RS Section 6.11)
    /// Uses ExceptionKind from patina-core for consistency.
    /// The irritants are serialized to a string for the error message.
    /// This can be caught by `guard` or `with-exception-handler`.
    #[error("Scheme exception ({kind:?}): {message}")]
    SchemeException {
        kind: ExceptionKind,
        message: String,
        irritants_display: String, // Serialized display of irritants
    },

    /// An error with an attached source location.
    ///
    /// Wraps another EvalError with a source position so that error messages
    /// can report "at source:line:col". Created by `err.at(loc)` or `err.at_opt(loc)`.
    #[error("{error}\n  at {location}")]
    WithLocation {
        #[source]
        error: Box<EvalError>,
        location: SourceLocation,
    },
}

impl EvalError {
    /// Wrap this error with a source location.
    pub fn at(self, loc: SourceLocation) -> Self {
        EvalError::WithLocation {
            error: Box::new(self),
            location: loc,
        }
    }

    /// Wrap this error with a source location if one is available.
    pub fn at_opt(self, loc: Option<SourceLocation>) -> Self {
        match loc {
            Some(loc) => self.at(loc),
            None => self,
        }
    }

    /// Return the source location attached to this error, if any.
    pub fn source_location(&self) -> Option<&SourceLocation> {
        match self {
            EvalError::WithLocation { location, .. } => Some(location),
            _ => None,
        }
    }

    /// Check if this error can be caught by Scheme exception handlers
    pub fn is_catchable(&self) -> bool {
        match self {
            EvalError::WithLocation { error, .. } => error.is_catchable(),
            _ => !matches!(
                self,
                EvalError::InternalError(_) | EvalError::ContinuationEscape
            ),
        }
    }

    /// Convert this error to an ErrorKind for classification
    pub fn to_error_kind(&self) -> ErrorKind {
        match self {
            EvalError::UndefinedVariable(_) => ErrorKind::Lookup,
            EvalError::NotAProcedure(_) => ErrorKind::Application,
            EvalError::WrongArity { .. } => ErrorKind::Arity,
            EvalError::InvalidSyntax(_) => ErrorKind::Syntax,
            EvalError::TypeError(_) => ErrorKind::Type,
            EvalError::DivisionByZero => ErrorKind::Domain,
            EvalError::IndexOutOfBounds(_) => ErrorKind::Bounds,
            EvalError::IOError(_) => ErrorKind::FileIO,
            EvalError::InternalError(_) => ErrorKind::Internal,
            EvalError::ContinuationEscape => ErrorKind::ControlFlow,
            EvalError::SchemeException { kind, .. } => match kind {
                ExceptionKind::FileError => ErrorKind::FileIO,
                ExceptionKind::ReadError => ErrorKind::Read,
                _ => ErrorKind::User,
            },
            EvalError::WithLocation { error, .. } => error.to_error_kind(),
        }
    }

    /// Convert to ErrorDetail for rich error reporting
    pub fn to_error_detail(&self) -> ErrorDetail {
        let kind = self.to_error_kind();
        match self {
            EvalError::UndefinedVariable(name) => ErrorDetail::lookup_error(name),
            EvalError::WrongArity { expected, actual } => {
                ErrorDetail::arity_error(expected, *actual)
            }
            EvalError::DivisionByZero => ErrorDetail::domain_error("division by zero"),
            EvalError::IOError(msg) => ErrorDetail::file_error(msg),
            EvalError::SchemeException {
                message,
                irritants_display,
                ..
            } => {
                let mut detail = ErrorDetail::new(kind, message);
                if !irritants_display.is_empty() {
                    // Note: We can't recover the actual Values from the display string,
                    // but we include it in the message
                    detail.message = format!("{} [{}]", message, irritants_display);
                }
                detail
            }
            EvalError::WithLocation { error, location } => {
                error.to_error_detail().with_location(location.clone())
            }
            _ => ErrorDetail::new(kind, self.to_string()),
        }
    }
}

// Convert ErrorDetail to EvalError
impl From<ErrorDetail> for EvalError {
    fn from(detail: ErrorDetail) -> Self {
        match detail.kind {
            ErrorKind::Lookup => EvalError::UndefinedVariable(detail.message),
            ErrorKind::Application => EvalError::NotAProcedure(detail.message),
            ErrorKind::Arity => EvalError::WrongArity {
                expected: "?".to_string(),
                actual: 0,
            },
            ErrorKind::Type => EvalError::TypeError(detail.message),
            ErrorKind::Domain => {
                if detail.message.contains("division by zero") {
                    EvalError::DivisionByZero
                } else {
                    EvalError::TypeError(detail.message)
                }
            }
            ErrorKind::Bounds => EvalError::IndexOutOfBounds(detail.message),
            ErrorKind::FileIO => EvalError::IOError(detail.message),
            ErrorKind::Read => {
                let irritants_display = detail.format_irritants();
                EvalError::SchemeException {
                    kind: ExceptionKind::ReadError,
                    message: detail.message,
                    irritants_display,
                }
            }
            ErrorKind::Syntax => EvalError::InvalidSyntax(detail.message),
            ErrorKind::User => {
                let irritants_display = detail.format_irritants();
                EvalError::SchemeException {
                    kind: ExceptionKind::Error,
                    message: detail.message,
                    irritants_display,
                }
            }
            ErrorKind::Internal => EvalError::InternalError(detail.message),
            ErrorKind::ControlFlow => EvalError::ContinuationEscape,
        }
    }
}
