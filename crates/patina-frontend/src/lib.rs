//! Patina Frontend - Lexer, Parser, Macro Expander, and Desugarer
//!
//! This crate contains the frontend components of Patina Scheme:
//!
//! - **Lexer** - Tokenizes Scheme source code
//! - **Parser** - Builds AST from tokens (as Value)
//! - **Macro Expander** - Expands syntax-rules macros hygienically
//! - **Desugarer** - Transforms surface syntax (Value) to core IR (CoreExpr)
//! - **cond-expand** - Feature requirement evaluation for conditional expansion
//!
//! # Architecture
//!
//! ```text
//! Source → Lexer → Tokens → Parser → Value → Macro Expander → Value → Desugarer → CoreExpr
//! ```

pub mod cond_expand;
pub mod desugarer;
pub mod error;
pub mod lexer;
pub mod library_parser;
pub mod macro_expander;
pub mod parser;

// Re-export main types
pub use cond_expand::evaluate_feature_requirement;
pub use desugarer::{DesugarError, Desugarer};
pub use error::FrontendError;
pub use lexer::{LexError, Lexer, Token};
pub use library_parser::{BodyElement, ExportSpec, ImportSet, LibraryDefinition};
pub use parser::{ParseError, Parser};
pub use patina_runtime::Value::Macro;
