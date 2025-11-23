//! Macro expansion system for Patina Scheme
//!
//! This crate provides the macro expansion infrastructure for Patina, including:
//! - R7RS `syntax-rules` macro system
//! - Hygienic macro expansion
//! - Pattern matching and template expansion
//! - Separate macro environment (decoupled from runtime)
//!
//! The macro system is based on PVREF (Pattern Variable Reference) encoding
//! for efficient O(1) variable lookup, inspired by Gauche Scheme's implementation.

pub mod error;
pub mod macro_expander;
pub mod tracer;

// Re-export main types at crate level
pub use error::{ExpansionStep, MacroError};
pub use macro_expander::{
    CompiledMacro, CompiledMacroExpander, CompiledRule, Compiler, ExpandError, Expander,
    ExpansionResult, Identifier, MacroExpander, MatchError, Matcher, Pattern, Template,
    TestExpander, apply_hygiene, expand_macro,
};
pub use tracer::MacroTracer;
