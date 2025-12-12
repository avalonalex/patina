//! Error types for pattern matching
//!
//! This module defines the error types returned when pattern matching fails.

/// Error type for pattern matching failures
#[derive(Debug, Clone, PartialEq)]
pub enum MatchError {
    /// Pattern requires more elements than input provides
    TooFewElements {
        pattern: String,
        expected: usize,
        actual: usize,
    },

    /// Input has more elements than pattern can match (no ellipsis to consume them)
    TooManyElements { expected: usize, actual: usize },

    /// Literal value doesn't match
    LiteralMismatch { expected: String, actual: String },

    /// Type mismatch (e.g., list pattern vs vector input)
    TypeMismatch { expected: String, actual: String },

    /// Vector pattern size doesn't match input vector size
    VectorSizeMismatch { expected: usize, actual: usize },

    /// Ellipsis pattern with inconsistent repetition counts
    InconsistentRepetition {
        var1: String,
        count1: usize,
        var2: String,
        count2: usize,
    },
}

impl std::fmt::Display for MatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchError::TooFewElements {
                pattern,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Pattern matching failed: {} requires at least {} element(s), but input has only {}\n\
                     Hint: Check that your macro call provides enough arguments",
                    pattern, expected, actual
                )
            }
            MatchError::TooManyElements { expected, actual } => {
                write!(
                    f,
                    "Pattern matching failed: expected {} element(s), got {}\n\
                     Hint: Pattern has no ellipsis (...) to consume extra elements. \
                     Either add '...' to the pattern or remove extra arguments",
                    expected, actual
                )
            }
            MatchError::LiteralMismatch { expected, actual } => {
                write!(
                    f,
                    "Pattern matching failed: literal mismatch\n\
                     Expected: {}\n\
                     Got:      {}\n\
                     Hint: Literals in patterns must match exactly",
                    expected, actual
                )
            }
            MatchError::TypeMismatch { expected, actual } => {
                write!(
                    f,
                    "Pattern matching failed: type mismatch\n\
                     Expected: {}\n\
                     Got:      {}\n\
                     Hint: List patterns only match lists, vector patterns only match vectors",
                    expected, actual
                )
            }
            MatchError::VectorSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "Pattern matching failed: vector size mismatch\n\
                     Expected: {} element(s)\n\
                     Got:      {} element(s)\n\
                     Hint: Vector patterns must match the exact number of elements (no ellipsis support yet)",
                    expected, actual
                )
            }
            MatchError::InconsistentRepetition {
                var1,
                count1,
                var2,
                count2,
            } => {
                write!(
                    f,
                    "Pattern matching failed: inconsistent repetition in ellipsis pattern\n\
                     Variable '{}' matched {} time(s)\n\
                     Variable '{}' matched {} time(s)\n\
                     Hint: All variables in the same ellipsis pattern must match the same number of times",
                    var1, count1, var2, count2
                )
            }
        }
    }
}

impl std::error::Error for MatchError {}
