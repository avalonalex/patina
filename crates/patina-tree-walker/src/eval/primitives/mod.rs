//! Primitive procedures dispatcher and installation
//!
//! All backend-agnostic primitives are implemented in the `patina-primitives` crate
//! and registered via `patina_primitives::register_all()`.
//!
//! Tree-walker-specific primitives (debug utilities) are in `debug.rs`.
//!
//! Note: `map` and `for-each` are implemented in Scheme (lib/scheme/base/higher_order.scm)
//! for proper CPS compatibility with call/cc.

mod debug;

use patina_core::TaggedValue;
use patina_runtime::Procedure;

use super::Evaluator;
use super::error::EvalError;

impl Evaluator {
    /// Dispatcher for primitive procedure calls with TaggedValue arguments
    pub(super) fn apply_primitive_tagged(
        &self,
        proc: &Procedure,
        args: Vec<TaggedValue>,
        _in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        // Extract pre-computed qualified name and index cache from the primitive
        let (name, qualified_name, registry_index) = match proc {
            Procedure::Primitive {
                name,
                qualified_name,
                registry_index,
                ..
            } => (*name, qualified_name.as_ref(), registry_index),
            _ => {
                return Err(EvalError::TypeError(
                    "apply_primitive_tagged called with non-primitive procedure".to_string(),
                ));
            }
        };

        // Use the shared patina-primitives registry
        self.primitive_registry
            .apply_cached(qualified_name, registry_index, &args, self)
            .map(super::EvalResult::Tagged)
            .map_err(|e| {
                if e.to_string().contains("not found") {
                    EvalError::InvalidSyntax(format!("Unknown primitive: {}", name))
                } else {
                    e
                }
            })
    }

    /// Register all primitive procedures in the registry
    pub(super) fn register_all_primitives(registry: &mut patina_primitives::PrimitiveRegistry) {
        // Register all backend-agnostic primitives from patina-primitives crate
        patina_primitives::register_all(registry);

        // Register tree-walker-specific debug primitives
        debug::register(registry);
    }
}
