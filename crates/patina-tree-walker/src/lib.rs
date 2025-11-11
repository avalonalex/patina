//! Patina Tree-Walking Interpreter
//!
//! This crate contains the tree-walking interpreter backend for Patina Scheme.
//!
//! # Components
//!
//! - **Evaluator** - Core evaluation engine (eval module)
//! - **Special Forms** - Built-in special forms (quote, lambda, if, etc.)
//! - **Primitives** - Built-in procedures organized by category
//!
//! # Architecture
//!
//! The tree-walking interpreter directly evaluates the AST produced by the frontend.
//! It uses lexical scoping with environment chains and supports full R7RS semantics.

pub mod eval;

// Re-export main types
pub use eval::{Evaluator, EvalError};
