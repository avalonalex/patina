//! Patina IR - Core Intermediate Representation
//!
//! This crate defines the Core IR that all compiler passes operate on.
//!
//! The Core IR is a minimal Scheme syntax with only 7 core forms:
//! - `quote` - Literal data
//! - `lambda` - Function abstraction
//! - `if` - Conditional (always ternary)
//! - `set!` - Mutation
//! - `begin` - Sequencing
//! - `define` - Top-level binding
//! - Application - Function calls
//!
//! All macros and derived forms (cond, case, let, and, or, etc.) are
//! desugared by the frontend before reaching the Core IR.

pub mod core_expr;
pub mod surface_syntax;
pub mod visitor;

// Re-export main types
pub use core_expr::{CoreExpr, Formals, Symbol};
pub use surface_syntax::SurfaceSyntax;
pub use visitor::ExprVisitor;
