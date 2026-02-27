//! Patina IR - Intermediate Representation utilities
//!
//! This crate provides IR utilities for the Patina compiler:
//! - Visitor patterns for IR transformation
//! - CPS transformation
//!
//! Core IR types (`CoreExpr`, `Formals`, `Symbol`) are defined in `patina-core`
//! and re-exported here for convenience.

pub mod cps_transform;
pub mod visitor;

// Re-export core IR types from patina-core
pub use patina_core::{CoreExpr, Formals, ScopedParam, Symbol};

// Re-export CPS types from patina-core
pub use patina_core::{CpsExpr, CpsParam, CpsPrimitive, PromptTag};

// Local types
pub use cps_transform::CpsTransformer;
pub use visitor::ExprVisitor;
