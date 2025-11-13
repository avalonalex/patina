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
pub mod library_parser;
pub mod macro_expander;
pub mod parser;

// Re-export main types
pub use error::FrontendError;
pub use lexer::{LexError, Lexer, Token};
pub use library_parser::{ExportSpec, ImportSet, LibraryDefinition};
pub use parser::{ParseError, Parser};
pub use patina_runtime::Value::Macro;
