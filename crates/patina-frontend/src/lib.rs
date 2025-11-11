//! Patina Frontend - Lexer, Parser, and Macro Expander
//!
//! This crate contains the frontend components of Patina Scheme:
//!
//! - **Lexer** - Tokenizes Scheme source code
//! - **Parser** - Builds AST from tokens (as Value)
//! - **Macro Expander** - Expands syntax-rules macros hygienically
//!
//! # Phase 1 Goal
//!
//! Extract frontend components into a separate crate for better organization.

pub mod error;
pub mod lexer;
pub mod parser;
pub mod macro_expander;

// Re-export main types
pub use error::FrontendError;
pub use lexer::{Lexer, Token, LexError};
pub use parser::{Parser, ParseError};
pub use macro_expander::Macro;
