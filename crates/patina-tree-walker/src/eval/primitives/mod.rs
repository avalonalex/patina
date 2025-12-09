//! Primitive procedures dispatcher and installation
//!
//! This module coordinates all primitive operations across different categories.
//! Each category is implemented in its own submodule:
//!
//! - `registry` - Primitive registry for runtime-extensible primitives
//! - `arithmetic` - Numeric operations (+, -, *, /, comparisons, etc.)
//! - `lists` - Pair and list operations (cons, car, cdr, append, etc.)
//! - `higher_order` - map, for-each
//! - `predicates` - Type predicates (number?, string?, etc.)
//! - `equality` - eq?, eqv?, equal?
//! - `values` - Multiple values support
//! - `strings` - String operations
//! - `vectors` - Vector operations
//! - `parameters` - Dynamic parameters (make-parameter)

pub mod registry;

mod arithmetic;
mod bytevectors;
mod characters;
mod conversion;
mod debug;
pub(in crate::eval) mod equality;
mod higher_order;
mod io;
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

use patina_runtime::value::{Procedure, Value};
use std::cell::RefCell;
use std::rc::Rc;

use super::Evaluator;
use super::error::EvalError;

impl Evaluator {
    /// Main dispatcher for primitive procedure calls
    ///
    /// The `in_tail_position` parameter indicates whether this primitive call is in tail context.
    /// Most primitives ignore this and return `EvalResult::Value`, but certain primitives like
    /// `call-with-values` can return `EvalResult::TailCallPrimitive` to participate in TCO.
    pub(super) fn apply_primitive(
        &self,
        proc: &Procedure,
        args: Vec<Value>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        // Extract name and library from the primitive
        let (name, library) = match proc {
            Procedure::Primitive { name, library, .. } => (*name, library),
            _ => {
                return Err(EvalError::TypeError(
                    "apply_primitive called with non-primitive procedure".to_string(),
                ));
            }
        };

        // Build qualified name using the procedure's library namespace
        let qualified_name = format!("{}/{}", library.join("."), name);

        // Apply the primitive and propagate any errors
        // (Don't swallow errors - if the primitive fails, return that error)
        self.primitive_registry
            .apply(&qualified_name, args, self, in_tail_position)
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
        conversion::register(registry);
        lists::register(registry);
        higher_order::register(registry);
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

        // All core primitives are now in the registry!
    }

    // ========== Helper Functions for Primitives ==========

    /// Check that exactly `expected` arguments were provided
    pub(in crate::eval) fn check_arity_exact(
        &self,
        args: &[Value],
        expected: usize,
        fn_name: &str,
    ) -> Result<(), EvalError> {
        if args.len() != expected {
            return Err(EvalError::WrongArity {
                expected: format!("{} expects {} argument(s)", fn_name, expected),
                actual: args.len(),
            });
        }
        Ok(())
    }

    /// Check that at least `min` arguments were provided
    pub(in crate::eval) fn check_arity_min(
        &self,
        args: &[Value],
        min: usize,
        fn_name: &str,
    ) -> Result<(), EvalError> {
        if args.len() < min {
            return Err(EvalError::WrongArity {
                expected: format!("{} expects at least {} argument(s)", fn_name, min),
                actual: args.len(),
            });
        }
        Ok(())
    }

    /// Check that between `min` and `max` arguments were provided
    pub(in crate::eval) fn check_arity_range(
        &self,
        args: &[Value],
        min: usize,
        max: usize,
        fn_name: &str,
    ) -> Result<(), EvalError> {
        if args.len() < min || args.len() > max {
            return Err(EvalError::WrongArity {
                expected: format!("{} expects {} to {} argument(s)", fn_name, min, max),
                actual: args.len(),
            });
        }
        Ok(())
    }

    /// Convert a Scheme list to a Vec, validating it's a proper list
    pub(in crate::eval) fn list_to_vec(
        &self,
        list: Value,
        fn_name: &str,
    ) -> Result<Vec<Value>, EvalError> {
        let mut items = Vec::new();
        let mut current = list;

        while let Value::Pair(pair) = current {
            let borrowed = pair.borrow();
            items.push(borrowed.0.clone());
            current = borrowed.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(format!(
                "{}: argument must be a proper list",
                fn_name
            )));
        }

        Ok(items)
    }

    /// Convert a Vec to a Scheme list
    pub(in crate::eval) fn list_from_vec(&self, items: Vec<Value>) -> Value {
        items.into_iter().rev().fold(Value::Null, |acc, item| {
            Value::Pair(Rc::new(RefCell::new((item, acc))))
        })
    }

    /// Generic type predicate helper
    pub(in crate::eval) fn make_type_predicate<F>(
        &self,
        args: Vec<Value>,
        predicate: F,
    ) -> Result<Value, EvalError>
    where
        F: Fn(&Value) -> bool,
    {
        self.check_arity_exact(&args, 1, "type predicate")?;
        Ok(Value::Boolean(predicate(&args[0])))
    }
}
