//! Patina Runtime - Core value types and environment model
//!
//! This crate provides the foundational runtime types used by all Patina components:
//! - `Value`: The Scheme value representation (numbers, lists, procedures, etc.)
//! - `Environment`: Lexical environment for variable bindings
//! - `Procedure` and `Arity`: Procedure representation
//!
//! This crate is intentionally minimal and stable - it should rarely change.

pub mod environment;
pub mod error;
pub mod library;
pub mod library_registry;
pub mod value;

// Re-export main types for convenience
pub use environment::Environment;
pub use error::RuntimeError;
pub use library::Library;
pub use library_registry::{LibraryError, LibraryRegistry};
pub use value::{Arity, Procedure, Value};
