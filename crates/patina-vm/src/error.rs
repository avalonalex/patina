//! Error types for the VM compiler and runtime.

use patina_core::core_expr::Symbol;
use patina_core::error::SourceLocation;
use thiserror::Error;

/// Errors produced during bytecode compilation (the 5-pass pipeline).
#[derive(Debug, Error)]
pub enum CompileError {
    #[error("unbound variable: `{}`", patina_core::escape_invisible(name))]
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

    /// A reference set-of-scopes resolution does not determine: two bindings
    /// are visible and neither is more specific. The message is the rule's
    /// own, from `patina_core::scope_resolve::AmbiguousReference`.
    #[error("{0}")]
    AmbiguousReference(String),

    /// A sub-expression the compiler had to desugar itself failed to desugar
    /// — the unquotes inside a quasiquote template are the only such site.
    /// The message is the desugarer's own, and `VmBackendError` reports it as
    /// a desugar error, so `` `(1 ,if) `` reads exactly like a bare `if` does.
    #[error("{0}")]
    Desugar(String),
}

/// Errors produced during VM execution.
#[derive(Debug, Error)]
pub enum VmError {
    #[error("unbound variable: `{}`", patina_core::escape_invisible(name))]
    UnboundVariable { name: Symbol },

    #[error("wrong number of arguments: expected {expected}, got {got}")]
    ArityMismatch { expected: String, got: usize },

    #[error("type error: {message}")]
    TypeError { message: String },

    #[error("no matching prompt tag for abort")]
    NoMatchingPrompt,

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

    /// A continuation was invoked and the frames it restored are not the ones
    /// the current Rust call chain was running. Never reaches a user: the
    /// value the continuation carries is parked on `VmState::pending_escape`
    /// and the dispatch loop that owns the resumed frame consumes both.
    ///
    /// Its own variant, and deliberately non-catchable, so that neither a
    /// `guard` nor a `?` on the way out can mistake an unwind for a program
    /// error. See `VmState::pending_escape`.
    #[error("continuation escaped past a synchronous boundary")]
    ContinuationEscape,

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
