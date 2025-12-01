//! Macro expansion system for Patina Scheme
//!
//! This crate provides the macro expansion infrastructure for Patina, including:
//! - R7RS `syntax-rules` macro system
//! - Hygienic macro expansion using Racket-style scope sets
//! - Pattern matching and template expansion
//! - Separate macro environment (decoupled from runtime)
//!
//! The macro system is based on PVREF (Pattern Variable Reference) encoding
//! for efficient O(1) variable lookup, inspired by Gauche Scheme's implementation.
//!
//! Hygiene is implemented using Racket's scope sets algorithm (based on
//! "Binding as Sets of Scopes" by Matthew Flatt, 2016), using flip-scope
//! operations to distinguish use-site vs introduced identifiers.

pub mod error;
pub mod macro_expander;
pub mod tracer;

// Re-export main types at crate level
pub use error::{ExpansionStep, MacroError};
pub use macro_expander::{
    CompiledMacro, CompiledMacroExpander, CompiledRule, Compiler, ExpandError, Expander,
    ExpansionResult, Identifier, MacroExpander, MatchError, Matcher, Pattern, Template,
    TestExpander, expand_macro, expand_macro_with_shadowed,
};
pub use tracer::MacroTracer;
