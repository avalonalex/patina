//! Patina Primitives - Shared primitive procedure implementations
//!
//! This crate provides backend-agnostic primitive procedure implementations
//! that can be used by both the tree-walker and the VM backend.
#![expect(
    clippy::disallowed_macros,
    reason = "the crate's one `thread_local!`, `io::ports`' `CURRENT_INPUT_PORT`, \
              `CURRENT_OUTPUT_PORT` and `CURRENT_ERROR_PORT`: Rust `Port`s, not heap values, per \
              thread, so two interpreters on one thread share them (#618). Clippy takes this lint \
              only at the crate root, so a new `thread_local!` in the crate goes on this list \
              (#622)"
)]

pub mod apply_context;
pub mod primitives;
pub mod registry;

pub use apply_context::ApplyContext;
pub use registry::{
    CallArgs, HOTaggedHandler, PrimitiveFn, PrimitiveHandler, PrimitiveRegistry, ResumableResume,
    ResumableStart, Step, TaggedHandler,
};

// Re-export EvalError for convenience
pub use patina_runtime::EvalError;

/// Register all shared primitives into the given registry
pub fn register_all(registry: &mut PrimitiveRegistry) {
    primitives::register_all(registry);
}
