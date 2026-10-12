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
// No unsafe code in the library; the binary allows its two `set_var` calls one by one
// (`PRD/GC_PRD.md` §11.3, #698).
#![forbid(unsafe_code)]

pub mod program_stream;
pub mod repl;

// Re-export main REPL type and shared editor infrastructure
pub use program_stream::run_program_stream;
pub use repl::Repl;
pub use repl::{Lines, SchemeHelper, run_repl_loop, session_lines};
