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
pub mod library_loader;
pub mod library_registry;
pub mod rust_library_loader;
pub mod stdlib;
pub mod value;

// Re-export main types for convenience
pub use environment::Environment;
pub use error::RuntimeError;
pub use library::Library;
pub use library_loader::{LibraryLoader, LibraryLoaderRegistry, RustLibraryBuilder};
pub use library_registry::{LibraryError, LibraryRegistry};
pub use rust_library_loader::RustLibraryLoader;
pub use value::{Arity, Procedure, Value};
