//! Error types for macro expansion

use thiserror::Error;

/// Errors that can occur during macro expansion
#[derive(Error, Debug, Clone)]
pub enum MacroError {
    /// Invalid syntax in macro definition or usage
    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    /// Pattern matching failed
    #[error("Pattern match failed: {0}")]
    MatchError(String),

    /// Template expansion failed
    #[error("Template expansion failed: {0}")]
    ExpansionError(String),

    /// Hygiene violation
    #[error("Hygiene violation: {0}")]
    HygieneError(String),

    /// No matching pattern for macro call
    #[error("No matching pattern for macro {0}")]
    NoMatchingPattern(String),
}
