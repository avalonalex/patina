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
//!   - `DesugarError`: The program was rejected before it ran

use patina_core::error::SourceLocation;
use patina_core::{ErrorDetail, ErrorKind, ExceptionKind};
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum EvalError {
    /// Reporting metadata only; the inner error still determines catchability
    /// and the Scheme condition delivered to an exception handler.
    #[error("{error}")]
    WithDiagnostic {
        error: Box<EvalError>,
        diagnostic: Box<crate::Diagnostic>,
    },

    #[error("Undefined variable: {}", patina_core::escape_invisible(_0))]
    UndefinedVariable(String),

    #[error("Not a procedure: {0}")]
    NotAProcedure(String),

    #[error("Wrong number of arguments: expected {expected}, got {actual}")]
    WrongArity { expected: String, actual: usize },

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    /// The desugarer rejected the form before evaluation began.
    ///
    /// `Backend::eval` receives raw datums and desugars them inside the
    /// backend, so a desugar failure has to travel through `EvalError`. This
    /// variant keeps its identity as a before-run rejection — wrapping it in
    /// `InternalError` (as the backend once did) mis-states the stage and
    /// reads as an interpreter bug. It is non-catchable because no handler
    /// can be installed before the program runs.
    ///
    /// **Produced only at the `Backend::eval` entry point.** A desugar
    /// failure reached from *inside* a running program — the `eval`
    /// primitive — is a runtime failure: use the catchable `InvalidSyntax`
    /// there, as the VM's equivalent path does.
    #[error("Desugar error: {0}")]
    DesugarError(String),

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

    /// A posted interrupt, Ctrl-C's, that nothing handled (#701). Raised at
    /// a poll as a non-continuable `&interrupt`, which is not an error
    /// object; a script that ends on one exits with status 130.
    #[error("interrupted")]
    Interrupt,

    /// A Scheme-level exception was raised (R7RS Section 6.11)
    /// Uses ExceptionKind from patina-core for consistency.
    /// The irritants are serialized to a string for the error message.
    /// This can be caught by `guard` or `with-exception-handler`.
    #[error("unhandled exception: {message}")]
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
    pub fn with_diagnostic(self, diagnostic: crate::Diagnostic) -> Self {
        if self.to_error_kind() == ErrorKind::ControlFlow {
            return self;
        }
        Self::WithDiagnostic {
            error: Box::new(self),
            diagnostic: Box::new(diagnostic),
        }
    }

    pub fn without_diagnostic(&self) -> &Self {
        match self {
            Self::WithDiagnostic { error, .. } => error.without_diagnostic(),
            _ => self,
        }
    }

    /// Wrap this error with a source location.
    pub fn at(self, loc: SourceLocation) -> Self {
        if self.source_location().is_some() {
            return self;
        }
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
            EvalError::WithDiagnostic { error, .. } => error.source_location(),
            _ => None,
        }
    }
}

impl patina_core::error::HasSourceLocation for EvalError {
    fn source_location(&self) -> Option<&SourceLocation> {
        self.source_location()
    }

    /// `WithLocation`'s `Display` appends `at <location>`, which a caller with
    /// no source map to format with still wants. The formatter prints the
    /// position itself, so it gets the wrapped error's text — every layer of
    /// it, since a located error can be located again on its way out.
    fn message_without_location(&self) -> String {
        match self {
            EvalError::WithLocation { error, .. } | EvalError::WithDiagnostic { error, .. } => {
                error.message_without_location()
            }
            other => other.to_string(),
        }
    }
}

impl EvalError {
    /// Check if this error can be caught by Scheme exception handlers
    pub fn is_catchable(&self) -> bool {
        match self {
            EvalError::WithLocation { error, .. } | EvalError::WithDiagnostic { error, .. } => {
                error.is_catchable()
            }
            _ => !matches!(
                self,
                EvalError::InternalError(_)
                    | EvalError::ContinuationEscape
                    | EvalError::DesugarError(_)
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
            // Syntax is for *reporting* only: catchability is decided by
            // stage (see is_catchable's blacklist), not by this kind, so
            // ErrorKind::is_catchable must not be consulted for this variant.
            EvalError::DesugarError(_) => ErrorKind::Syntax,
            EvalError::TypeError(_) => ErrorKind::Type,
            EvalError::DivisionByZero => ErrorKind::Domain,
            EvalError::IndexOutOfBounds(_) => ErrorKind::Bounds,
            EvalError::IOError(_) => ErrorKind::FileIO,
            EvalError::InternalError(_) => ErrorKind::Internal,
            EvalError::ContinuationEscape => ErrorKind::ControlFlow,
            EvalError::Interrupt => ErrorKind::Interrupt,
            EvalError::SchemeException { kind, .. } => match kind {
                ExceptionKind::FileError => ErrorKind::FileIO,
                ExceptionKind::ReadError => ErrorKind::Read,
                ExceptionKind::Interrupt => ErrorKind::Interrupt,
                _ => ErrorKind::User,
            },
            EvalError::WithLocation { error, .. } | EvalError::WithDiagnostic { error, .. } => {
                error.to_error_kind()
            }
        }
    }

    /// The condition this error raises in Scheme: its kind, and the message
    /// `error-object-message` answers, with no display prefix. Both machines
    /// raise an error through it (#744): the kind comes from the variant, so
    /// a type error is never a file error however its message reads, and
    /// only an I/O error's message is searched for a file's, and a syntax
    /// error's for a parse's.
    pub fn to_exception(&self) -> (ExceptionKind, String) {
        match self.without_diagnostic() {
            // Lookup errors
            EvalError::UndefinedVariable(name) => (
                ExceptionKind::Error,
                format!("Undefined variable: {}", name),
            ),

            // Application errors
            EvalError::NotAProcedure(desc) => {
                (ExceptionKind::Error, format!("Not a procedure: {}", desc))
            }

            // Arity errors
            EvalError::WrongArity { expected, actual } => (
                ExceptionKind::Error,
                format!(
                    "Wrong number of arguments: expected {}, got {}",
                    expected, actual
                ),
            ),

            // Type errors
            EvalError::TypeError(msg) => (ExceptionKind::Error, msg.clone()),

            // Domain errors
            EvalError::DivisionByZero => (ExceptionKind::Error, "Division by zero".to_string()),

            // Bounds errors
            EvalError::IndexOutOfBounds(msg) => (ExceptionKind::Error, msg.clone()),

            // I/O errors - classify as file-error when appropriate
            EvalError::IOError(msg) => {
                let kind = if msg.contains("Cannot open")
                    || msg.contains("Cannot delete")
                    || msg.contains("Cannot read")
                    || msg.contains("Cannot write")
                    || msg.contains("No such file")
                    || msg.contains("file")
                {
                    ExceptionKind::FileError
                } else {
                    ExceptionKind::Error
                };
                (kind, msg.clone())
            }

            // Syntax/read errors
            EvalError::InvalidSyntax(msg) => {
                let kind = if msg.contains("read:") || msg.contains("parse") {
                    ExceptionKind::ReadError
                } else {
                    ExceptionKind::Error
                };
                (kind, msg.clone())
            }

            // A posted interrupt, raised at a poll (#701)
            EvalError::Interrupt => (ExceptionKind::Interrupt, "interrupted".to_string()),

            // Already a Scheme exception - use its kind
            EvalError::SchemeException {
                kind,
                message,
                irritants_display: _,
            } => (kind.clone(), message.clone()),

            // Located error: include location in exception message
            EvalError::WithLocation { error, location } => {
                let detail = error.to_error_detail();
                let msg = format!("{} at {}", detail.message, location);
                (detail.kind.to_exception_kind(), msg)
            }

            // Not catchable on the tree-walker, which never asks; the VM catches
            // a primitive's, as an ordinary error with its display.
            error @ (EvalError::WithDiagnostic { .. }
            | EvalError::InternalError(_)
            | EvalError::ContinuationEscape
            | EvalError::DesugarError(_)) => (ExceptionKind::Error, error.to_string()),
        }
    }

    /// Convert to ErrorDetail for rich error reporting
    pub fn to_error_detail(&self) -> ErrorDetail {
        if let Self::WithDiagnostic { error, .. } = self {
            return error.to_error_detail();
        }
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
            ErrorKind::Interrupt => EvalError::Interrupt,
        }
    }
}

impl crate::HasDiagnostic for EvalError {
    fn diagnostic(&self) -> crate::Diagnostic {
        use crate::{Diagnostic, DiagnosticKind as K};
        match self {
            Self::WithDiagnostic { diagnostic, .. } => (**diagnostic).clone(),
            Self::WithLocation { error, .. } => error.diagnostic(),
            Self::UndefinedVariable(name) => {
                let mut d = Diagnostic::new(K::UnboundIdentifier, self.to_string());
                d.identifier = Some(name.clone());
                d
            }
            Self::DesugarError(_) => Diagnostic::new(K::Syntax, self.to_string()),
            Self::Interrupt
            | Self::SchemeException {
                kind: ExceptionKind::Interrupt,
                ..
            } => Diagnostic::new(K::Interrupt, self.to_string()),
            _ => Diagnostic::new(K::Runtime, self.to_string()),
        }
    }
}
