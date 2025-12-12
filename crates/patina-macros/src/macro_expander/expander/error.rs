//! Error types for template expansion
//!
//! This module defines the error types returned when template expansion fails.

/// Error type for template expansion failures
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandError {
    /// Undefined pattern variable referenced in template
    UndefinedVariable { pvref: String },

    /// Variable used at wrong ellipsis level
    LevelMismatch {
        pvref: String,
        template_level: usize,
        actual_level: usize,
    },

    /// Ellipsis iteration with inconsistent repetition counts
    InconsistentRepetition { expected: usize, actual: usize },

    /// Invalid template structure
    InvalidTemplate { message: String },
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpandError::UndefinedVariable { pvref } => {
                write!(
                    f,
                    "Template expansion failed: undefined pattern variable: {}\n\
                     Hint: This variable was not bound during pattern matching. \
                     Check that the variable appears in the macro pattern",
                    pvref
                )
            }
            ExpandError::LevelMismatch {
                pvref,
                template_level,
                actual_level,
            } => {
                write!(
                    f,
                    "Template expansion failed: ellipsis level mismatch\n\
                     Variable:       {}\n\
                     Template level: {}\n\
                     Actual level:   {}\n\
                     Hint: Variable must be used at the same ellipsis nesting depth where it was bound",
                    pvref, template_level, actual_level
                )
            }
            ExpandError::InconsistentRepetition { expected, actual } => {
                write!(
                    f,
                    "Template expansion failed: inconsistent ellipsis repetition\n\
                     Expected: {} iteration(s)\n\
                     Got:      {} iteration(s)\n\
                     Hint: All variables in the same ellipsis template must have the same repetition count",
                    expected, actual
                )
            }
            ExpandError::InvalidTemplate { message } => {
                write!(
                    f,
                    "Template expansion failed: invalid template structure\n\
                     Error: {}\n\
                     Hint: Check the macro template syntax",
                    message
                )
            }
        }
    }
}

impl std::error::Error for ExpandError {}
