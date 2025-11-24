//! Error types for the desugarer

use patina_runtime::Value;
use std::fmt;

/// Errors that can occur during desugaring
#[derive(Debug, Clone)]
pub enum DesugarError {
    /// Invalid syntax for a special form
    InvalidSyntax(String),

    /// Wrong number of arguments
    WrongArgCount {
        form: String,
        expected: String,
        got: usize,
    },

    /// Empty body (e.g., empty lambda, begin, or define)
    EmptyBody(String),

    /// Runtime-only value appeared in AST
    RuntimeValueInAST { value: Value, context: String },

    /// Expected a proper list but got improper list or non-list
    ExpectedProperList(String),

    /// Duplicate parameter names
    DuplicateParameter { name: String, context: String },

    /// Invalid formal parameter syntax
    InvalidFormals(String),

    /// Special form that requires Value evaluator fallback (not in CoreExpr yet)
    /// This is temporary until all forms are migrated to CoreExpr
    FallbackFormNeeded { form: String },

    /// Generic error with message
    Other(String),
}

impl fmt::Display for DesugarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DesugarError::InvalidSyntax(msg) => {
                write!(f, "Invalid syntax: {}", msg)
            }
            DesugarError::WrongArgCount {
                form,
                expected,
                got,
            } => {
                write!(f, "{} expects {} arguments, got {}", form, expected, got)
            }
            DesugarError::EmptyBody(form) => {
                write!(f, "{} body cannot be empty", form)
            }
            DesugarError::RuntimeValueInAST { value, context } => {
                write!(
                    f,
                    "Runtime-only value {:?} cannot appear in AST: {}",
                    value, context
                )
            }
            DesugarError::ExpectedProperList(context) => {
                write!(f, "Expected proper list: {}", context)
            }
            DesugarError::DuplicateParameter { name, context } => {
                write!(f, "Duplicate parameter '{}' in {}", name, context)
            }
            DesugarError::InvalidFormals(msg) => {
                write!(f, "Invalid formal parameters: {}", msg)
            }
            DesugarError::FallbackFormNeeded { form } => {
                write!(f, "{} requires Value evaluator (not yet in CoreExpr)", form)
            }
            DesugarError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for DesugarError {}

/// Convenience result type
pub type Result<T> = std::result::Result<T, DesugarError>;
