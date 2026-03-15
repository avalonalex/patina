//! Error types for the VM compiler and runtime.

use patina_core::core_expr::Symbol;
use patina_core::error::SourceLocation;
use thiserror::Error;

/// Errors produced during bytecode compilation (the 5-pass pipeline).
#[derive(Debug, Error)]
pub enum CompileError {
    #[error("unbound variable: `{name}`")]
    UnboundVariable { name: Symbol },

    #[error("invalid syntax at {location}: {message}")]
    InvalidSyntax {
        message: String,
        location: SourceLocation,
    },

    #[error("too many registers required ({count}); maximum is 65535")]
    TooManyRegisters { count: usize },

    #[error("internal compiler error: {0}")]
    Internal(String),
}

/// Errors produced during VM execution.
#[derive(Debug, Error)]
pub enum VmError {
    #[error("unbound variable: `{name}`")]
    UnboundVariable { name: Symbol },

    #[error("wrong number of arguments: expected {expected}, got {got}")]
    ArityMismatch { expected: String, got: usize },

    #[error("type error: {message}")]
    TypeError { message: String },

    #[error("no matching prompt tag for abort")]
    NoMatchingPrompt,

    #[error("continuation invoked with wrong number of values")]
    ContinuationValueMismatch,

    #[error("divide by zero")]
    DivideByZero,

    #[error("stack overflow")]
    StackOverflow,

    #[error("compile error: {0}")]
    Compile(#[from] CompileError),

    #[error("runtime error: {message}")]
    Runtime { message: String },

    /// A Scheme exception that wasn't caught by any handler.
    #[error("unhandled exception: {message}")]
    SchemeException { message: String },

    /// Wraps an inner error with a source location for error reporting.
    #[error("{error}")]
    WithLocation {
        #[source]
        error: Box<VmError>,
        location: SourceLocation,
    },
}

impl VmError {
    /// Wrap this error with a source location.
    pub fn at(self, loc: SourceLocation) -> Self {
        VmError::WithLocation {
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
            VmError::WithLocation { location, .. } => Some(location),
            _ => None,
        }
    }

    /// Get the innermost error, stripping any location wrappers.
    pub fn inner(&self) -> &VmError {
        match self {
            VmError::WithLocation { error, .. } => error.inner(),
            other => other,
        }
    }
}
