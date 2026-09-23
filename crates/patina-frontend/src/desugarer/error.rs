//! Error types for the desugarer

use patina_core::{ErrorDetail, ErrorKind, SourceLocation};
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
    RuntimeValueInAST { type_name: String, context: String },

    /// Expected a proper list but got improper list or non-list
    ExpectedProperList(String),

    /// Duplicate parameter names
    DuplicateParameter { name: String, context: String },

    /// Invalid formal parameter syntax
    InvalidFormals(String),

    /// A reference set-of-scopes resolution does not determine.
    ///
    /// Two bindings are visible and neither is more specific than the other,
    /// so no answer is justified. Raised here rather than at runtime because
    /// it is a property of the program's binding structure, which is settled
    /// once expansion is done. See `patina_core::scope_resolve`.
    AmbiguousReference(String),

    /// Generic error with message
    Other(String),

    /// Another desugar error, with where in the source it was raised.
    ///
    /// Attached once, by the innermost form that has a position
    /// (`Desugarer::desugar_form`), so the error points at the form that was
    /// wrong rather than at the top-level form it sat in. An error raised
    /// while a macro expanded is placed at the use site, since the template's
    /// pairs are stamped with it. Its text is the wrapped error's; the
    /// position reaches the user through `source_location`, which the backends
    /// carry to `format_interpreter_error` (#432).
    WithLocation {
        error: Box<DesugarError>,
        location: SourceLocation,
    },
}

impl DesugarError {
    /// Place this error at `location`, unless it already has a position — the
    /// innermost form that knows its place is the more precise answer.
    pub fn at_opt(self, location: Option<SourceLocation>) -> Self {
        match (location, self.source_location()) {
            (Some(location), None) => DesugarError::WithLocation {
                error: Box::new(self),
                location,
            },
            _ => self,
        }
    }

    /// Where in the source this error was raised, if that is known.
    pub fn source_location(&self) -> Option<&SourceLocation> {
        match self {
            DesugarError::WithLocation { location, .. } => Some(location),
            _ => None,
        }
    }

    /// Get the error kind for classification
    pub fn to_error_kind(&self) -> ErrorKind {
        match self {
            DesugarError::InvalidSyntax(_) => ErrorKind::Syntax,
            DesugarError::WrongArgCount { .. } => ErrorKind::Arity,
            DesugarError::EmptyBody(_) => ErrorKind::Syntax,
            DesugarError::RuntimeValueInAST { .. } => ErrorKind::Internal,
            DesugarError::ExpectedProperList(_) => ErrorKind::Syntax,
            DesugarError::DuplicateParameter { .. } => ErrorKind::Syntax,
            DesugarError::InvalidFormals(_) => ErrorKind::Syntax,
            DesugarError::AmbiguousReference(_) => ErrorKind::Syntax,
            DesugarError::Other(_) => ErrorKind::Internal,
            DesugarError::WithLocation { error, .. } => error.to_error_kind(),
        }
    }

    /// Convert to ErrorDetail for rich error reporting
    ///
    /// Note: Source location will be None until SOURCE_INFO_PLAN.md is implemented.
    pub fn to_error_detail(&self) -> ErrorDetail {
        ErrorDetail::new(self.to_error_kind(), self.to_string())
    }
}

impl fmt::Display for DesugarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DesugarError::InvalidSyntax(msg) => {
                write!(f, "Invalid syntax: {}", msg)
            }
            DesugarError::AmbiguousReference(msg) => {
                write!(f, "{}", msg)
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
            DesugarError::RuntimeValueInAST { type_name, context } => {
                write!(
                    f,
                    "Runtime-only value of type '{}' cannot appear in AST: {}",
                    type_name, context
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
            DesugarError::Other(msg) => write!(f, "{}", msg),
            DesugarError::WithLocation { error, .. } => write!(f, "{}", error),
        }
    }
}

impl std::error::Error for DesugarError {}

// Conversion to ErrorDetail
impl From<DesugarError> for ErrorDetail {
    fn from(e: DesugarError) -> Self {
        e.to_error_detail()
    }
}

/// Convenience result type
pub type Result<T> = std::result::Result<T, DesugarError>;
