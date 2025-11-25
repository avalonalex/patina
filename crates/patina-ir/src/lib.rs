//! Patina IR - Intermediate Representation utilities
//!
//! This crate provides IR utilities for the Patina compiler:
//! - Visitor patterns for IR transformation
//! - Surface syntax helpers
//!
//! Core IR types (`CoreExpr`, `Formals`, `Symbol`) are defined in `patina-core`
//! and re-exported here for convenience.

pub mod surface_syntax;
pub mod visitor;

// Re-export core IR types from patina-core
pub use patina_core::core_expr::CaseLambdaClause;
pub use patina_core::{CoreExpr, Formals, Primitive, Symbol};

// Local types
pub use surface_syntax::SurfaceSyntax;
pub use visitor::ExprVisitor;
