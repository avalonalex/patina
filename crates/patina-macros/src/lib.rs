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
    CompiledMacro, CompiledRule, Compiler, EllipsisBinding, ExpandError, Expander, Identifier,
    IdentifierKey, MatchError, Matcher, ParsedSyntaxRules, Pattern, SyntaxRulesParseError,
    Template, TestExpander, expand_macro_with_shadowed_tagged, parse_syntax_rules,
};
pub use tracer::MacroTracer;
