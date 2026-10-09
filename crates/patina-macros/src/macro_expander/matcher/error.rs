//! Error types for pattern matching
//!
//! This module defines the error types returned when pattern matching fails.

use patina_core::{Heap, TaggedValue};

/// How a mismatch names the input it refused, for the macro debug log, the
/// one reader of a failed attempt at a rule (`DebugContext::log_match_failure`):
/// written only while that log is on, and then in brief (#617).
///
/// Every rule tried before the one that matches fails, and formatting the
/// input in full each time was work for every expansion, and recursion as
/// deep as the input: what a `cond` clause is matched against can be the rest
/// of a program nested thousands deep, and writing it out overflowed the
/// stack.
pub(super) fn described(input: TaggedValue, heap: &Heap) -> String {
    if patina_runtime::macro_debug::is_enabled() {
        patina_core::format_tagged_brief(input, heap)
    } else {
        String::new()
    }
}

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

    /// Internal error (programming error or unsupported case)
    InternalError(String),

    /// A literal comparison whose resolution is ambiguous. Not a failed match:
    /// trying the next rule would settle the reference by rule order, so the
    /// expansion stops here.
    AmbiguousLiteral(String),
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
            MatchError::InternalError(msg) => {
                write!(f, "Internal pattern matching error: {}", msg)
            }
            MatchError::AmbiguousLiteral(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for MatchError {}
