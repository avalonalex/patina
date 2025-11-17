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

pub mod registry;

mod arithmetic;
mod debug;
pub(in crate::eval) mod equality;
mod higher_order;
mod io;
mod lists;
mod predicates;
mod strings;
mod values;
mod vectors;

// Re-export registry types for convenience
pub use registry::PrimitiveRegistry;

use patina_runtime::value::{Procedure, Value};
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
        if let Ok(result) =
            self.primitive_registry
                .apply(&qualified_name, args.clone(), self, in_tail_position)
        {
            return Ok(result);
        }

        // Fall back to match statement for primitives not yet converted to registry
        // NOTE: All arithmetic operations are now in the registry (scheme.base/+, -, *, /, etc.)
        // NOTE: All list operations are now in the registry (scheme.base/cons, car, cdr, list, etc.)
        // NOTE: All higher-order functions are now in the registry (scheme.base/map, for-each)
        // NOTE: All type predicates are now in the registry (scheme.base/number?, boolean?, etc.)
        // NOTE: All equality operations are now in the registry (scheme.base/eq?, eqv?, equal?)
        // NOTE: All string operations are now in the registry (scheme.base/string-length, string-ref, etc.)
        // NOTE: All vector operations are now in the registry (scheme.base/make-vector, vector-ref, etc.)
        // NOTE: All multiple value operations are now in the registry (scheme.base/values, call-with-values)
        // NOTE: All I/O operations are now in the registry (scheme.base/display, write, newline)
        //
        // TODO: Migrate remaining primitives to registry when namespace/import system is implemented:
        // - Debug primitives should be in patina.debug namespace (debug-enable, debug-disable, etc.)
        // - Test framework primitives should be in patina.test namespace (test-begin, test-end, etc.)
        // - Macro debug should be in patina.debug namespace (macro-debug-mode)
        // Currently, apply_primitive only checks scheme.base namespace, so these would need
        // multi-namespace lookup support or proper import/namespace resolution.
        match name {
            // Debug primitives (patina.debug namespace - not yet migrated)
            "debug-enable" => debug::debug_enable(self, args).map(super::EvalResult::Value),
            "debug-disable" => debug::debug_disable(self, args).map(super::EvalResult::Value),
            "debug-clear" => debug::debug_clear(self, args).map(super::EvalResult::Value),
            "debug-status" => debug::debug_status(self, args).map(super::EvalResult::Value),
            "debug-mode" => debug::debug_mode(self, args).map(super::EvalResult::Value),

            // Testing framework (chibi test)
            "test-begin" => {
                self.check_arity_exact(&args, 1, "test-begin")?;
                let name = match &args[0] {
                    Value::String(s) => s.borrow().clone(),
                    Value::Symbol(s) => s.to_string(),
                    _ => {
                        return Err(EvalError::TypeError(
                            "test-begin expects string or symbol".to_string(),
                        ));
                    }
                };
                patina_runtime::stdlib::test_begin(&name);
                Ok(super::EvalResult::Value(Value::Unspecified))
            }
            "test-end" => {
                patina_runtime::stdlib::test_end();
                Ok(super::EvalResult::Value(Value::Unspecified))
            }
            "test-increment-passed" => {
                patina_runtime::stdlib::test_increment_passed();
                Ok(super::EvalResult::Value(Value::Unspecified))
            }
            "test-increment-failed" => {
                patina_runtime::stdlib::test_increment_failed();
                Ok(super::EvalResult::Value(Value::Unspecified))
            }
            "macro-debug-mode" => {
                self.check_arity_exact(&args, 1, "macro-debug-mode")?;
                match &args[0] {
                    Value::Symbol(s) if s.as_ref() == "on" => {
                        patina_runtime::macro_debug::enable();
                        Ok(super::EvalResult::Value(Value::Symbol(
                            "macro-debug-enabled".into(),
                        )))
                    }
                    Value::Symbol(s) if s.as_ref() == "off" => {
                        patina_runtime::macro_debug::disable();
                        Ok(super::EvalResult::Value(Value::Symbol(
                            "macro-debug-disabled".into(),
                        )))
                    }
                    Value::Symbol(s) if s.as_ref() == "status" => {
                        let status = if patina_runtime::macro_debug::is_enabled() {
                            "enabled"
                        } else {
                            "disabled"
                        };
                        Ok(super::EvalResult::Value(Value::Symbol(status.into())))
                    }
                    _ => Err(EvalError::InvalidSyntax(
                        "macro-debug-mode expects 'on, 'off, or 'status".to_string(),
                    )),
                }
            }

            _ => Err(EvalError::InvalidSyntax(format!(
                "Unknown primitive: {}",
                name
            ))),
        }
    }

    /// Register all primitive procedures in the registry
    ///
    /// This is the new registry-based approach for primitives.
    /// Each category module will provide a registration function.
    pub(super) fn register_all_primitives(registry: &mut PrimitiveRegistry) {
        // Register primitives by category
        arithmetic::register(registry);
        lists::register(registry);
        higher_order::register(registry);
        predicates::register(registry);
        equality::register(registry);
        strings::register(registry);
        vectors::register(registry);
        values::register(registry);
        io::register(registry);

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
            items.push(pair.0.clone());
            current = pair.1.clone();
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
        items
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, item| Value::Pair(Rc::new((item, acc))))
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
