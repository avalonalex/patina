//! Patina Runtime - Runtime services for Patina Scheme interpreter
//!
//! This crate provides runtime services and re-exports core types from `patina-core`:
//!
//! **From patina-core (re-exported for convenience):**
//! - `TaggedValue`: Compact NaN-boxed value representation
//! - `Heap`, `SharedHeap`: Arena-based heap for objects
//! - `Environment`: Lexical environment for variable bindings
//! - `Procedure` and `Arity`: Procedure representation
//! - `ScopeId`, `ScopeSet`: Scope tracking for hygiene
//! - `PVRef`, `MatchEnv`, `MatchValue`: Macro pattern variables
//! - `CoreExpr`, `Formals`: Core IR types
//! - `Library`: R7RS library representation
//!
//! **Runtime services (defined here):**
//! - `Backend`: Trait for pluggable interpreter implementations
//! - `LibraryRegistry`, `LibraryLoader`: Library management
//! - Standard library implementations

// Runtime-specific modules
pub mod backend;
pub mod error;
pub mod eval_error;
pub mod features;
pub mod library_loader;
pub mod library_registry;
pub mod macro_debug;
pub mod rust_library_loader;
pub mod stdlib;

// Re-export all types from patina-core
pub use patina_core::*;

// Re-export runtime-specific types
pub use backend::Backend;
pub use error::RuntimeError;
pub use eval_error::EvalError;
pub use features::{FeatureRegistry, default_features};
pub use library_loader::{LibraryLoader, LibraryLoaderRegistry, RustLibraryBuilder};
pub use library_registry::{LibraryError, LibraryRegistry, NATIVE_EXTENSION_MARKER};
pub use rust_library_loader::RustLibraryLoader;
