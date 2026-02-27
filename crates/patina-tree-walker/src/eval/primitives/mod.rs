//! Primitive procedures dispatcher and installation
//!
//! This module coordinates all primitive operations across different categories.
//! Each category is implemented in its own submodule:
//!
//! - `registry` - Primitive registry for runtime-extensible primitives
//! - `arithmetic` - Numeric operations (+, -, *, /, comparisons, etc.)
//! - `lists` - Pair and list operations (cons, car, cdr, append, etc.)
//! - `predicates` - Type predicates (number?, string?, etc.)
//! - `equality` - eq?, eqv?, equal?
//! - `values` - Multiple values support
//! - `strings` - String operations
//! - `vectors` - Vector operations
//! - `parameters` - Dynamic parameters (make-parameter)
//!
//! Note: `map` and `for-each` are implemented in Scheme (lib/scheme/base/higher_order.scm)
//! for proper CPS compatibility with call/cc.

pub mod registry;

mod arithmetic;
mod bytevectors;
mod characters;
mod continuations;
mod conversion;
mod debug;
pub(in crate::eval) mod equality;
mod eval;
mod exceptions;
pub mod io;
mod lazy;
mod lists;
mod parameters;
mod predicates;
mod process_context;
mod records;
mod strings;
mod symbols;
mod system;
mod test;
mod time;
mod values;
mod vectors;

// Re-export registry types for convenience
pub use registry::PrimitiveRegistry;

use patina_core::TaggedValue;
use patina_runtime::Procedure;

use super::Evaluator;
use super::error::EvalError;

impl Evaluator {
    /// Dispatcher for primitive procedure calls with TaggedValue arguments
    ///
    /// The `in_tail_position` parameter indicates whether this primitive call is in tail context.
    /// Most primitives ignore this and return `EvalResult::Tagged`, but certain primitives like
    /// `call-with-values` can return `EvalResult::TailCallPrimitive` to participate in TCO.
    pub(super) fn apply_primitive_tagged(
        &self,
        proc: &Procedure,
        args: Vec<TaggedValue>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        // Extract pre-computed qualified name from the primitive
        let (name, qualified_name) = match proc {
            Procedure::Primitive {
                name,
                qualified_name,
                ..
            } => (*name, qualified_name.as_ref()),
            _ => {
                return Err(EvalError::TypeError(
                    "apply_primitive_tagged called with non-primitive procedure".to_string(),
                ));
            }
        };

        // Use pre-computed qualified name — no format!() allocation needed
        self.primitive_registry
            .apply_tagged(qualified_name, args, self, in_tail_position)
            .map_err(|e| {
                // If the error is "primitive not found", give a clearer message
                if e.to_string().contains("not found") {
                    EvalError::InvalidSyntax(format!("Unknown primitive: {}", name))
                } else {
                    e
                }
            })
    }

    /// Register all primitive procedures in the registry
    ///
    /// This is the new registry-based approach for primitives.
    /// Each category module will provide a registration function.
    pub(super) fn register_all_primitives(registry: &mut PrimitiveRegistry) {
        // Register primitives by category
        arithmetic::register(registry);
        bytevectors::register(registry);
        characters::register(registry);
        continuations::register(registry);
        conversion::register(registry);
        lists::register(registry);
        predicates::register(registry);
        equality::register(registry);
        strings::register(registry);
        symbols::register(registry);
        vectors::register(registry);
        values::register(registry);
        io::register(registry);
        debug::register(registry);
        test::register(registry);
        lazy::register(registry);
        parameters::register(registry);
        system::register(registry);
        time::register(registry);
        process_context::register(registry);
        records::register(registry);
        eval::register(registry);
        exceptions::register(registry);

        // All core primitives are now in the registry!
    }
}
