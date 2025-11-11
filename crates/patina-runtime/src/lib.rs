//! Patina Runtime - Core value types and environment model
//!
//! This crate provides the foundational runtime types used by all Patina components:
//! - `Value`: The Scheme value representation (numbers, lists, procedures, etc.)
//! - `Environment`: Lexical environment for variable bindings
//! - `Procedure` and `Arity`: Procedure representation
//!
//! This crate is intentionally minimal and stable - it should rarely change.

pub mod value;
pub mod environment;
pub mod error;

// Re-export main types for convenience
pub use value::{Value, Procedure, Arity};
pub use environment::Environment;
pub use error::RuntimeError;
