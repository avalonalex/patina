//! Patina Tree-Walking Interpreter
//!
//! This crate contains the tree-walking interpreter backend for Patina Scheme.
//!
//! # Components
//!
//! - **TreeWalker** - Backend trait implementation wrapping the evaluator
//! - **Evaluator** - Core evaluation engine (eval module)
//! - **Special Forms** - Built-in special forms (quote, lambda, if, etc.)
//! - **Primitives** - Built-in procedures organized by category
//!
//! # Architecture
//!
//! The tree-walking interpreter directly evaluates the AST produced by the frontend.
//! It uses lexical scoping with environment chains and supports full R7RS semantics.
//!
//! # Using as a Backend
//!
//! ```ignore
//! use patina_tree_walker::TreeWalker;
//! use patina_runtime::Backend;
//!
//! let backend = TreeWalker::new();
//! let result = backend.eval_global(&expr)?;
//! ```

pub mod backend;
pub mod eval;
pub mod library_support;

// Re-export main types
pub use backend::{EvalMode, TreeWalker};
pub use eval::{eval_core, eval_cps, CpsEvaluator, EvalError, Evaluator};
pub use library_support::SchemeLibraryLoader;
