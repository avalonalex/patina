//! Macro expansion system for Patina Scheme
//!
//! This crate provides the macro expansion infrastructure for Patina, including:
//! - R7RS `syntax-rules` macro system
//! - Hygienic macro expansion using marks-and-ribs algorithm
//! - Pattern matching and template expansion
//! - Separate macro environment (decoupled from runtime)
//!
//! The macro system is based on PVREF (Pattern Variable Reference) encoding
//! for efficient O(1) variable lookup, inspired by Gauche Scheme's implementation.
//!
//! Hygiene is implemented using the marks-and-ribs algorithm from Chez Scheme,
//! which tracks expansion phases using marks attached to identifiers.

pub mod error;
pub mod macro_expander;
pub mod marks_and_ribs;
pub mod tracer;

// Re-export main types at crate level
pub use error::{ExpansionStep, MacroError};
pub use macro_expander::{
    CompiledMacro, CompiledMacroExpander, CompiledRule, Compiler, ExpandError, Expander,
    ExpansionResult, Identifier, MacroExpander, MatchError, Matcher, Pattern, Template,
    TestExpander, expand_macro,
};
pub use marks_and_ribs::{Mark, MarkList, MarksAndRibsHygiene, Rib, WrappedIdentifier};
pub use tracer::MacroTracer;
