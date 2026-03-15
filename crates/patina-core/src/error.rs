//! Unified error types for Patina Scheme interpreter
//!
//! This module provides a consistent error handling foundation across all Patina components.
//! It defines:
//! - `ExceptionKind`: R7RS exception classification
//! - `ErrorKind`: Classification of all possible errors
//! - `SourceLocation`: Source file location for error reporting
//! - `ErrorDetail`: Rich error context including location and irritants

use std::fmt;
use std::sync::Arc;

use crate::tagged_value::TaggedValue;

// =============================================================================
// Exception Types (R7RS Section 6.11)
// =============================================================================

/// The kind of exception
///
/// R7RS defines specific predicates for different exception types:
/// - `error-object?` - any error object created by `error`
/// - `file-error?` - I/O errors related to files
/// - `read-error?` - errors during parsing/reading
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExceptionKind {
    /// Generic error created by `(error message irritant ...)`
    Error,
    /// File I/O error (maps from EvalError::IOError)
    /// Satisfies `file-error?` predicate
    FileError,
    /// Read/parse error (maps from parse errors)
    /// Satisfies `read-error?` predicate
    ReadError,
    /// User-defined exception kind
    Custom(String),
}

/// Exception object for R7RS exception handling
///
/// Created by the `error` procedure or by converting internal errors.
/// Can be caught by `guard` or `with-exception-handler`.
#[derive(Debug, Clone)]
pub struct ExceptionObject {
    /// The kind of exception (error, file-error, read-error, etc.)
    pub kind: ExceptionKind,
    /// The error message string
    pub message: String,
    /// Additional values related to the error ("irritants" in R7RS terminology)
    pub irritants: Vec<TaggedValue>,
}

// =============================================================================
// Source Location
// =============================================================================

/// Source location for error reporting
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// Source file, "<repl>", or "<string>"
    pub source: Arc<str>,
    /// 1-indexed line number
    pub line: u32,
    /// 1-indexed column number
    pub column: u32,
    /// Optional span length
    pub length: Option<u32>,
}

impl SourceLocation {
    /// Create a new source location
    pub fn new(source: impl AsRef<str>, line: u32, column: u32) -> Self {
        Self {
            source: Arc::from(source.as_ref()),
            line,
            column,
            length: None,
        }
    }

    /// Create a source location with a span length
    pub fn with_length(source: impl AsRef<str>, line: u32, column: u32, length: u32) -> Self {
        Self {
            source: Arc::from(source.as_ref()),
            line,
            column,
            length: Some(length),
        }
    }

    /// Create a location for REPL input
    pub fn repl(line: u32, column: u32) -> Self {
        Self::new("<repl>", line, column)
    }

    /// Create a location for string input
    pub fn string(line: u32, column: u32) -> Self {
        Self::new("<string>", line, column)
    }
}

/// Trait for error types that may carry a source location.
///
/// Enables generic source-aware error formatting across different backends.
pub trait HasSourceLocation {
    fn source_location(&self) -> Option<&SourceLocation>;
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.source, self.line, self.column)
    }
}

// =============================================================================
// Error Classification
// =============================================================================

/// Classification of errors
///
/// This enum determines:
/// 1. Whether the error is catchable in Scheme (`guard`, `with-exception-handler`)
/// 2. Which R7RS predicate matches (`error-object?`, `file-error?`, `read-error?`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    // === Catchable errors (become Scheme exceptions) ===
    /// Type mismatch: expected X, got Y
    Type,

    /// Wrong number of arguments
    Arity,

    /// Domain error (div by zero, invalid argument value)
    Domain,

    /// Index/key out of bounds
    Bounds,

    /// Variable not found
    Lookup,

    /// Tried to call non-procedure
    Application,

    /// File I/O error - maps to `file-error?` predicate
    FileIO,

    /// Read/parse error - maps to `read-error?` predicate
    Read,

    /// Syntax error (macro expansion, invalid special form)
    Syntax,

    /// User-raised error via `(error ...)`
    User,

    // === Non-catchable errors (stay in Rust) ===
    /// Internal interpreter bug - should never happen
    Internal,

    /// Control flow mechanism, not a real error
    ControlFlow,
}

impl ErrorKind {
    /// Is this error catchable by Scheme's `guard`/`with-exception-handler`?
    pub fn is_catchable(&self) -> bool {
        !matches!(self, ErrorKind::Internal | ErrorKind::ControlFlow)
    }

    /// Map to R7RS `ExceptionKind` for Scheme-level handling
    pub fn to_exception_kind(&self) -> ExceptionKind {
        match self {
            ErrorKind::FileIO => ExceptionKind::FileError,
            ErrorKind::Read => ExceptionKind::ReadError,
            _ => ExceptionKind::Error,
        }
    }

    /// Get a short description of this error kind
    pub fn description(&self) -> &'static str {
        match self {
            ErrorKind::Type => "type error",
            ErrorKind::Arity => "arity error",
            ErrorKind::Domain => "domain error",
            ErrorKind::Bounds => "bounds error",
            ErrorKind::Lookup => "lookup error",
            ErrorKind::Application => "application error",
            ErrorKind::FileIO => "file error",
            ErrorKind::Read => "read error",
            ErrorKind::Syntax => "syntax error",
            ErrorKind::User => "error",
            ErrorKind::Internal => "internal error",
            ErrorKind::ControlFlow => "control flow",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

// =============================================================================
// Error Detail
// =============================================================================

/// Detailed error information that can be converted to Scheme exception
///
/// This struct captures all relevant context for an error:
/// - The kind of error (for classification)
/// - Human-readable message
/// - Related values (irritants in R7RS terminology) as TaggedValues
/// - Source location (if available)
#[derive(Debug, Clone)]
pub struct ErrorDetail {
    /// The kind of error
    pub kind: ErrorKind,
    /// Human-readable message
    pub message: String,
    /// Values related to the error (irritants in R7RS)
    pub irritants: Vec<TaggedValue>,
    /// Where the error occurred
    pub location: Option<SourceLocation>,
}

impl ErrorDetail {
    /// Create a new error detail
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            irritants: vec![],
            location: None,
        }
    }

    /// Add an irritant value
    pub fn with_irritant(mut self, value: TaggedValue) -> Self {
        self.irritants.push(value);
        self
    }

    /// Add multiple irritant values
    pub fn with_irritants(mut self, values: Vec<TaggedValue>) -> Self {
        self.irritants = values;
        self
    }

    /// Add source location
    pub fn with_location(mut self, location: SourceLocation) -> Self {
        self.location = Some(location);
        self
    }

    /// Add optional source location
    pub fn with_opt_location(mut self, location: Option<SourceLocation>) -> Self {
        self.location = location;
        self
    }

    /// Check if this error is catchable in Scheme
    pub fn is_catchable(&self) -> bool {
        self.kind.is_catchable()
    }

    /// Format message with location if available
    pub fn format_message(&self) -> String {
        match &self.location {
            Some(loc) => format!("{}: {}", loc, self.message),
            None => self.message.clone(),
        }
    }

    /// Format irritants for display using TaggedValue's Display impl.
    /// For full-fidelity formatting of heap objects, use datum_writer with a Heap reference.
    pub fn format_irritants(&self) -> String {
        self.irritants
            .iter()
            .map(|tv| format!("{}", tv))
            .collect::<Vec<_>>()
            .join(" ")
    }

    // === Convenience constructors ===

    /// Create a type error with a TaggedValue irritant
    pub fn type_error(message: impl Into<String>, got: TaggedValue) -> Self {
        Self::new(ErrorKind::Type, message).with_irritant(got)
    }

    /// Create an arity error
    pub fn arity_error(expected: &str, actual: usize) -> Self {
        Self::new(
            ErrorKind::Arity,
            format!("expected {} arguments, got {}", expected, actual),
        )
        .with_irritant(TaggedValue::fixnum(actual as i64))
    }

    /// Create a domain error (e.g., division by zero)
    pub fn domain_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Domain, message)
    }

    /// Create a bounds error
    pub fn bounds_error(index: i64, len: usize) -> Self {
        Self::new(
            ErrorKind::Bounds,
            format!("index {} out of bounds for length {}", index, len),
        )
        .with_irritants(vec![
            TaggedValue::fixnum(index),
            TaggedValue::fixnum(len as i64),
        ])
    }

    /// Create a lookup error (undefined variable)
    pub fn lookup_error(name: &str) -> Self {
        Self::new(ErrorKind::Lookup, format!("undefined variable: {}", name))
    }

    /// Create an application error (not a procedure)
    pub fn application_error(desc: &str) -> Self {
        Self::new(ErrorKind::Application, format!("not a procedure: {}", desc))
    }

    /// Create a file I/O error
    pub fn file_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::FileIO, message)
    }

    /// Create a read/parse error
    pub fn read_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Read, message)
    }

    /// Create a syntax error
    pub fn syntax_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Syntax, message)
    }

    /// Create a user error (from Scheme's `error` procedure)
    pub fn user_error(message: impl Into<String>, irritants: Vec<TaggedValue>) -> Self {
        Self::new(ErrorKind::User, message).with_irritants(irritants)
    }

    /// Create an internal error (interpreter bug)
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl fmt::Display for ErrorDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(loc) = &self.location {
            write!(f, "{}: ", loc)?;
        }
        write!(f, "{}: {}", self.kind, self.message)?;
        if !self.irritants.is_empty() {
            write!(f, " [{}]", self.format_irritants())?;
        }
        Ok(())
    }
}

impl std::error::Error for ErrorDetail {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_kind_catchable() {
        assert!(ErrorKind::Type.is_catchable());
        assert!(ErrorKind::Arity.is_catchable());
        assert!(ErrorKind::Domain.is_catchable());
        assert!(ErrorKind::FileIO.is_catchable());
        assert!(ErrorKind::Read.is_catchable());

        assert!(!ErrorKind::Internal.is_catchable());
        assert!(!ErrorKind::ControlFlow.is_catchable());
    }

    #[test]
    fn test_error_kind_to_exception_kind() {
        assert_eq!(ErrorKind::Type.to_exception_kind(), ExceptionKind::Error);
        assert_eq!(
            ErrorKind::FileIO.to_exception_kind(),
            ExceptionKind::FileError
        );
        assert_eq!(
            ErrorKind::Read.to_exception_kind(),
            ExceptionKind::ReadError
        );
    }

    #[test]
    fn test_source_location_display() {
        let loc = SourceLocation::new("test.scm", 10, 5);
        assert_eq!(format!("{}", loc), "test.scm:10:5");

        let repl_loc = SourceLocation::repl(1, 3);
        assert_eq!(format!("{}", repl_loc), "<repl>:1:3");
    }

    #[test]
    fn test_error_detail_type_error() {
        let err = ErrorDetail::type_error("expected number", TaggedValue::TRUE);

        assert_eq!(err.kind, ErrorKind::Type);
        assert!(err.message.contains("expected number"));
        assert_eq!(err.irritants.len(), 1);
        assert_eq!(err.irritants[0], TaggedValue::TRUE);
    }

    #[test]
    fn test_error_detail_with_location() {
        let err =
            ErrorDetail::lookup_error("foo").with_location(SourceLocation::new("test.scm", 5, 10));

        assert!(err.format_message().contains("test.scm:5:10"));
        assert!(err.format_message().contains("undefined variable: foo"));
    }
}
