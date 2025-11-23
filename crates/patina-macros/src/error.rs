//! Error types for macro expansion

use std::rc::Rc;
use thiserror::Error;

/// A step in the macro expansion trace
///
/// Records information about a single macro expansion for debugging purposes.
#[derive(Debug, Clone)]
pub struct ExpansionStep {
    /// Name of the macro being expanded
    pub macro_name: Rc<str>,

    /// Which rule matched (0-indexed)
    pub rule_index: usize,

    /// Total number of rules tried
    pub total_rules: usize,

    /// The input form that was matched
    pub input: String,
}

impl ExpansionStep {
    pub fn new(macro_name: Rc<str>, rule_index: usize, total_rules: usize, input: String) -> Self {
        Self {
            macro_name,
            rule_index,
            total_rules,
            input,
        }
    }
}

impl std::fmt::Display for ExpansionStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "  {} (rule {}/{}): {}",
            self.macro_name,
            self.rule_index + 1,
            self.total_rules,
            self.input
        )
    }
}

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

    /// Error with expansion context for better debugging
    #[error("{message}")]
    WithContext {
        message: String,
        /// The macro that was being expanded when the error occurred
        macro_name: Option<Rc<str>>,
        /// The expansion trace showing how we got here
        expansion_trace: Vec<ExpansionStep>,
    },
}

impl MacroError {
    /// Add expansion context to an error
    pub fn with_context(self, macro_name: Rc<str>, expansion_trace: Vec<ExpansionStep>) -> Self {
        match self {
            // If already has context, just return it
            MacroError::WithContext { .. } => self,
            // Otherwise, wrap with context
            other => {
                let message = other.to_string();
                MacroError::WithContext {
                    message,
                    macro_name: Some(macro_name),
                    expansion_trace,
                }
            }
        }
    }

    /// Format error with full expansion trace
    pub fn format_with_trace(&self) -> String {
        match self {
            MacroError::WithContext {
                message,
                macro_name,
                expansion_trace,
            } => {
                let mut output = format!("Macro expansion error: {}", message);

                if let Some(name) = macro_name {
                    output.push_str(&format!("\n  in macro: {}", name));
                }

                if !expansion_trace.is_empty() {
                    output.push_str("\n\nExpansion trace:");
                    for step in expansion_trace {
                        output.push_str(&format!("\n{}", step));
                    }
                }

                output
            }
            other => other.to_string(),
        }
    }
}
