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
}

/// Errors produced during VM execution.
#[derive(Debug, Error)]
pub enum VmError {
    /// Reporting metadata only; the inner error still determines catchability
    /// and the Scheme condition delivered to an exception handler.
    #[error("{error}")]
    WithDiagnostic {
        error: Box<VmError>,
        diagnostic: Box<patina_runtime::Diagnostic>,
    },

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
    pub fn with_diagnostic(self, diagnostic: patina_runtime::Diagnostic) -> Self {
        Self::WithDiagnostic {
            error: Box::new(self),
            diagnostic: Box::new(diagnostic),
        }
    }

    /// Wrap this error with a source location.
    pub fn at(self, loc: SourceLocation) -> Self {
        if self.source_location().is_some() {
            return self;
        }
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
            VmError::WithDiagnostic { error, .. } => error.source_location(),
            _ => None,
        }
    }

    /// Get the innermost error, stripping any location wrappers.
    pub fn inner(&self) -> &VmError {
        match self {
            VmError::WithLocation { error, .. } | VmError::WithDiagnostic { error, .. } => {
                error.inner()
            }
            other => other,
        }
    }
}

impl patina_runtime::HasDiagnostic for CompileError {
    fn diagnostic(&self) -> patina_runtime::Diagnostic {
        use patina_runtime::{Diagnostic, DiagnosticKind as K};
        let mut d = Diagnostic::new(K::Runtime, self.to_string());
        match self {
            Self::UnboundVariable { name } => {
                d.kind = K::UnboundIdentifier;
                d.identifier = Some(name.to_string());
            }
            Self::InvalidSyntax { .. } | Self::AmbiguousReference(_) => d.kind = K::Syntax,
            _ => {}
        }
        d
    }
}

impl patina_runtime::HasDiagnostic for VmError {
    fn diagnostic(&self) -> patina_runtime::Diagnostic {
        use patina_runtime::{Diagnostic, DiagnosticKind as K};
        match self {
            Self::WithDiagnostic { diagnostic, .. } => (**diagnostic).clone(),
            Self::WithLocation { error, .. } => error.diagnostic(),
            Self::Compile(error) => error.diagnostic(),
            Self::UnboundVariable { name } => {
                let mut d = Diagnostic::new(K::UnboundIdentifier, self.to_string());
                d.identifier = Some(name.to_string());
                d
            }
            _ => Diagnostic::new(K::Runtime, self.to_string()),
        }
    }
}
