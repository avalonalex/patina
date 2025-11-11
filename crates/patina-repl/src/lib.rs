//! Patina REPL - Interactive Read-Eval-Print Loop
//!
//! This crate provides the REPL (Read-Eval-Print Loop) for Patina Scheme.
//!
//! # Features
//!
//! - **Syntax highlighting** - Real-time highlighting of Scheme code
//! - **Multi-line editing** - Balanced parenthesis detection
//! - **History** - Command history saved to ~/.patina_history
//! - **Rich terminal** - Built on rustyline for a modern CLI experience

pub mod repl;

// Re-export main REPL type
pub use repl::Repl;
