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

use patina_runtime::environment::Environment;
use patina_runtime::value::{Arity, Procedure, Value};
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
        name: &str,
        args: Vec<Value>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        // Try registry first with scheme.base namespace
        // This allows primitives registered in the registry to take precedence
        //
        // TODO: This hardcodes "scheme.base" which is a temporary hack!
        // In the future, Value::Primitive should store the library namespace,
        // so we can look up primitives from any library (scheme.char, scheme.file, etc.)
        // For now, all primitives are from scheme.base, so this works.
        let qualified_name = format!("scheme.base/{}", name);
        if let Ok(result) =
            self.primitive_registry
                .apply(&qualified_name, args.clone(), self, in_tail_position)
        {
            return Ok(result);
        }

        // Fall back to match statement for primitives not yet converted to registry
        // Most primitives just return their value wrapped in EvalResult::Value
        // Only special primitives like call-with-values use in_tail_position
        match name {
            // Special case: call-with-values can participate in tail call optimization
            "call-with-values" => values::call_with_values(self, args, in_tail_position),

            // All other primitives ignore tail position and return Value
            // NOTE: All arithmetic operations are now in the registry (scheme.base/+, -, *, /, etc.)
            // NOTE: All list operations are now in the registry (scheme.base/cons, car, cdr, list, etc.)
            // NOTE: All higher-order functions are now in the registry (scheme.base/map, for-each)
            // NOTE: All type predicates are now in the registry (scheme.base/number?, boolean?, etc.)
            // NOTE: All equality operations are now in the registry (scheme.base/eq?, eqv?, equal?)
            // NOTE: All string operations are now in the registry (scheme.base/string-length, string-ref, etc.)
            // NOTE: All vector operations are now in the registry (scheme.base/make-vector, vector-ref, etc.)

            // Multiple values
            "values" => values::values(self, args).map(super::EvalResult::Value),

            // I/O operations
            "display" => io::display(self, args).map(super::EvalResult::Value),
            "write" => io::write(self, args).map(super::EvalResult::Value),
            "newline" => io::newline(self, args).map(super::EvalResult::Value),

            // Debug primitives
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

        // All core primitives are now in the registry!
    }

    /// Install all primitive procedures into the environment
    ///
    /// This is the old environment-based approach, kept for backward compatibility
    /// during the migration to the registry system.
    pub(super) fn install_primitives(env: &Rc<Environment>) {
        let primitives = [
            // Arithmetic
            ("+", Arity::Min(0)),
            ("-", Arity::Min(1)),
            ("*", Arity::Min(0)),
            ("/", Arity::Min(1)),
            ("=", Arity::Min(2)),
            ("<", Arity::Min(2)),
            (">", Arity::Min(2)),
            ("<=", Arity::Min(2)),
            (">=", Arity::Min(2)),
            ("quotient", Arity::Exact(2)),
            ("remainder", Arity::Exact(2)),
            ("modulo", Arity::Exact(2)),
            ("abs", Arity::Exact(1)),
            ("max", Arity::Min(1)),
            ("min", Arity::Min(1)),
            ("floor", Arity::Exact(1)),
            ("ceiling", Arity::Exact(1)),
            ("truncate", Arity::Exact(1)),
            ("round", Arity::Exact(1)),
            ("sqrt", Arity::Exact(1)),
            ("square", Arity::Exact(1)),
            ("expt", Arity::Exact(2)),
            ("finite?", Arity::Exact(1)),
            ("infinite?", Arity::Exact(1)),
            ("nan?", Arity::Exact(1)),
            ("sin", Arity::Exact(1)),
            ("cos", Arity::Exact(1)),
            ("tan", Arity::Exact(1)),
            ("asin", Arity::Exact(1)),
            ("acos", Arity::Exact(1)),
            ("atan", Arity::Range(1, 2)),
            ("exp", Arity::Exact(1)),
            ("log", Arity::Range(1, 2)),
            ("gcd", Arity::Min(0)),
            ("lcm", Arity::Min(0)),
            ("numerator", Arity::Exact(1)),
            ("denominator", Arity::Exact(1)),
            ("exact", Arity::Exact(1)),
            ("inexact", Arity::Exact(1)),
            ("real-part", Arity::Exact(1)),
            ("imag-part", Arity::Exact(1)),
            ("magnitude", Arity::Exact(1)),
            ("angle", Arity::Exact(1)),
            ("make-rectangular", Arity::Exact(2)),
            ("make-polar", Arity::Exact(2)),
            ("exact-integer-sqrt", Arity::Exact(1)),
            ("rationalize", Arity::Exact(2)),
            // Lists
            ("cons", Arity::Exact(2)),
            ("car", Arity::Exact(1)),
            ("cdr", Arity::Exact(1)),
            ("list", Arity::Min(0)),
            ("length", Arity::Exact(1)),
            ("append", Arity::Min(0)),
            ("reverse", Arity::Exact(1)),
            ("list-ref", Arity::Exact(2)),
            ("list-tail", Arity::Exact(2)),
            ("memq", Arity::Exact(2)),
            ("memv", Arity::Exact(2)),
            ("member", Arity::Range(2, 3)),
            ("assq", Arity::Exact(2)),
            ("assv", Arity::Exact(2)),
            ("assoc", Arity::Range(2, 3)),
            // Higher-order
            ("map", Arity::Min(2)),
            ("for-each", Arity::Min(2)),
            // Predicates
            ("number?", Arity::Exact(1)),
            ("complex?", Arity::Exact(1)),
            ("real?", Arity::Exact(1)),
            ("rational?", Arity::Exact(1)),
            ("integer?", Arity::Exact(1)),
            ("boolean?", Arity::Exact(1)),
            ("string?", Arity::Exact(1)),
            ("symbol?", Arity::Exact(1)),
            ("null?", Arity::Exact(1)),
            ("pair?", Arity::Exact(1)),
            ("list?", Arity::Exact(1)),
            ("exact?", Arity::Exact(1)),
            ("inexact?", Arity::Exact(1)),
            ("boolean=?", Arity::Min(2)),
            ("procedure?", Arity::Exact(1)),
            ("char?", Arity::Exact(1)),
            ("vector?", Arity::Exact(1)),
            ("exact-integer?", Arity::Exact(1)),
            ("library?", Arity::Exact(1)),
            // Equality
            ("eq?", Arity::Exact(2)),
            ("eqv?", Arity::Exact(2)),
            ("equal?", Arity::Exact(2)),
            // Multiple values
            ("values", Arity::Min(0)),
            ("call-with-values", Arity::Exact(2)),
            // Strings
            ("string-length", Arity::Exact(1)),
            ("string-ref", Arity::Exact(2)),
            ("string-set!", Arity::Exact(3)),
            ("make-string", Arity::Range(1, 2)),
            ("string", Arity::Min(0)),
            ("string=?", Arity::Min(2)),
            ("string<?", Arity::Min(2)),
            ("string>?", Arity::Min(2)),
            ("string<=?", Arity::Min(2)),
            ("string>=?", Arity::Min(2)),
            ("string-ci=?", Arity::Min(2)),
            ("string-ci<?", Arity::Min(2)),
            ("string-ci>?", Arity::Min(2)),
            ("string-ci<=?", Arity::Min(2)),
            ("string-ci>=?", Arity::Min(2)),
            ("string-append", Arity::Min(0)),
            ("substring", Arity::Exact(3)),
            ("string->list", Arity::Range(1, 3)),
            ("list->string", Arity::Exact(1)),
            ("string-copy", Arity::Range(1, 3)),
            // Vectors
            ("make-vector", Arity::Range(1, 2)),
            ("vector", Arity::Min(0)),
            ("vector-length", Arity::Exact(1)),
            ("vector-ref", Arity::Exact(2)),
            ("vector-set!", Arity::Exact(3)),
            ("vector->list", Arity::Range(1, 3)),
            ("list->vector", Arity::Exact(1)),
            ("vector->string", Arity::Range(1, 3)),
            ("string->vector", Arity::Range(1, 3)),
            ("vector-copy", Arity::Range(1, 3)),
            ("vector-copy!", Arity::Range(3, 5)),
            ("vector-append", Arity::Min(0)),
            ("vector-fill!", Arity::Range(2, 4)),
            ("vector-map", Arity::Min(2)),
            ("vector-for-each", Arity::Min(2)),
            // I/O
            ("display", Arity::Exact(1)),
            ("write", Arity::Exact(1)),
            ("newline", Arity::Exact(0)),
            // Debug
            ("debug-enable", Arity::Exact(1)),
            ("debug-disable", Arity::Exact(1)),
            ("debug-clear", Arity::Exact(0)),
            ("debug-status", Arity::Exact(0)),
            ("debug-mode", Arity::Exact(1)),
            ("macro-debug-mode", Arity::Exact(1)),
            // Test framework
            ("test-begin", Arity::Exact(1)),
            ("test-end", Arity::Exact(0)),
            ("test-increment-passed", Arity::Exact(0)),
            ("test-increment-failed", Arity::Exact(0)),
        ];

        for (name, arity) in primitives {
            env.define(
                name.to_string(),
                Value::Procedure(Procedure::Primitive { name, arity }),
            );
        }
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
