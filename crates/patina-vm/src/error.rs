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
}
