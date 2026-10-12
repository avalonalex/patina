//! Patina IR - Intermediate Representation utilities
//!
//! This crate provides IR utilities for the Patina compiler:
//! - Visitor patterns for IR transformation
//! - CPS transformation
//!
//! Core IR types (`CoreExpr`, `Formals`, `Symbol`) are defined in `patina-core`
//! and re-exported here for convenience.
// No unsafe code here: unsafe heap access is confined to `patina-core`, `patina-vm` and
// `patina-tree-walker` (`PRD/GC_PRD.md` §11.3, #698).
#![forbid(unsafe_code)]

pub mod cps_transform;
pub mod visitor;

// Re-export core IR types from patina-core
pub use patina_core::{
    CoreExpr, CoreExprKind, Formals, QuasiConstructor, QuasiTemplate, ScopedParam, Symbol,
};

// Re-export CPS types from patina-core
pub use patina_core::{CpsExpr, CpsExprKind, CpsParam, CpsPrimitive, PromptTag};

// Local types
pub use cps_transform::CpsTransformer;
pub use visitor::{ExprVisitor, validate_executable};
