//! Patina Primitives - Shared primitive procedure implementations
//!
//! This crate provides backend-agnostic primitive procedure implementations
//! that can be used by both the tree-walker and the VM backend.
// No unsafe code here: unsafe heap access is confined to `patina-core`, `patina-vm` and
// `patina-tree-walker` (`PRD/GC_PRD.md` §11.3, #698).
#![forbid(unsafe_code)]

pub mod apply_context;
pub mod primitives;
pub mod registry;

pub use apply_context::ApplyContext;
pub use registry::{
    CallArgs, CxHandler, HigherOrderHandler, HostPrimitives, PrimClass, PrimitiveFn,
    PrimitiveHandler, PrimitiveRegistry, ResumableResume, ResumableStart, Step, StepFn,
    TaggedCallArgs, TaggedStep,
};

// Re-export EvalError for convenience
pub use patina_runtime::EvalError;

/// Register all shared primitives into the given registry
pub fn register_all(registry: &mut PrimitiveRegistry) {
    primitives::register_all(registry);
}
