//! Patina Primitives - Shared primitive procedure implementations
//!
//! This crate provides backend-agnostic primitive procedure implementations
//! that can be used by both the tree-walker and the VM backend.

pub mod apply_context;
pub mod primitives;
pub mod registry;

pub use apply_context::ApplyContext;
pub use registry::{
    HOTaggedHandler, PrimitiveFn, PrimitiveHandler, PrimitiveRegistry, TaggedHandler,
};

// Re-export EvalError for convenience
pub use patina_runtime::EvalError;

/// Register all shared primitives into the given registry
pub fn register_all(registry: &mut PrimitiveRegistry) {
    primitives::register_all(registry);
}
