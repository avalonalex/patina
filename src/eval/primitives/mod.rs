//! Primitive procedures dispatcher and installation
//!
//! This module coordinates all primitive operations across different categories.
//! Each category is implemented in its own submodule:
//!
//! - `arithmetic` - Numeric operations (+, -, *, /, comparisons, etc.)
//! - `lists` - Pair and list operations (cons, car, cdr, append, etc.)
//! - `higher_order` - map, for-each
//! - `predicates` - Type predicates (number?, string?, etc.)
//! - `equality` - eq?, eqv?, equal?
//! - `values` - Multiple values support
//! - `strings` - String operations
//! - `vectors` - Vector operations

mod arithmetic;
mod debug;
pub(in crate::eval) mod equality;
mod higher_order;
mod lists;
mod predicates;
mod strings;
mod values;
mod vectors;

use crate::env::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

use super::error::EvalError;
use super::Evaluator;

impl Evaluator {
    /// Main dispatcher for primitive procedure calls
    pub(super) fn apply_primitive(&self, name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
        match name {
            // Arithmetic operations
            "+" => arithmetic::add(self, args),
            "-" => arithmetic::subtract(self, args),
            "*" => arithmetic::multiply(self, args),
            "/" => arithmetic::divide(self, args),
            "=" => arithmetic::numeric_equal(self, args),
            "<" => arithmetic::less_than(self, args),
            ">" => arithmetic::greater_than(self, args),
            "<=" => arithmetic::less_equal(self, args),
            ">=" => arithmetic::greater_equal(self, args),
            "quotient" => arithmetic::quotient(self, args),
            "remainder" => arithmetic::remainder(self, args),
            "modulo" => arithmetic::modulo(self, args),
            "abs" => arithmetic::abs(self, args),
            "max" => arithmetic::max(self, args),
            "min" => arithmetic::min(self, args),

            // Pair/List operations
            "cons" => lists::cons(self, args),
            "car" => lists::car(self, args),
            "cdr" => lists::cdr(self, args),
            "list" => Ok(self.list_from_vec(args)),
            "length" => lists::length(self, args),
            "append" => lists::append(self, args),
            "reverse" => lists::reverse(self, args),
            "list-ref" => lists::list_ref(self, args),
            "list-tail" => lists::list_tail(self, args),
            "memq" => lists::memq(self, args),
            "memv" => lists::memv(self, args),
            "member" => lists::member(self, args),
            "assq" => lists::assq(self, args),
            "assv" => lists::assv(self, args),
            "assoc" => lists::assoc(self, args),

            // Higher-order functions
            "map" => higher_order::map(self, args),
            "for-each" => higher_order::for_each(self, args),

            // Type predicates
            "number?" => predicates::number_p(self, args),
            "integer?" => predicates::integer_p(self, args),
            "boolean?" => predicates::boolean_p(self, args),
            "string?" => predicates::string_p(self, args),
            "symbol?" => predicates::symbol_p(self, args),
            "null?" => predicates::null_p(self, args),
            "pair?" => predicates::pair_p(self, args),
            "list?" => predicates::list_p(self, args),
            "exact?" => predicates::exact_p(self, args),
            "inexact?" => predicates::inexact_p(self, args),
            "boolean=?" => predicates::boolean_equal(self, args),
            "procedure?" => predicates::procedure_p(self, args),
            "char?" => predicates::char_p(self, args),
            "vector?" => predicates::vector_p(self, args),

            // Equality operations
            "eq?" => equality::eq(self, args),
            "eqv?" => equality::eqv(self, args),
            "equal?" => equality::equal(self, args),

            // Multiple values
            "values" => values::values(self, args),
            "call-with-values" => values::call_with_values(self, args),

            // String operations
            "string-length" => strings::string_length(self, args),
            "string-ref" => strings::string_ref(self, args),
            "string-set!" => strings::string_set(self, args),
            "make-string" => strings::make_string(self, args),
            "string" => strings::string(self, args),
            "string=?" => strings::string_equal(self, args),
            "string<?" => strings::string_less(self, args),
            "string>?" => strings::string_greater(self, args),
            "string<=?" => strings::string_less_equal(self, args),
            "string>=?" => strings::string_greater_equal(self, args),
            "string-ci=?" => strings::string_ci_equal(self, args),
            "string-ci<?" => strings::string_ci_less(self, args),
            "string-ci>?" => strings::string_ci_greater(self, args),
            "string-ci<=?" => strings::string_ci_less_equal(self, args),
            "string-ci>=?" => strings::string_ci_greater_equal(self, args),
            "string-append" => strings::string_append(self, args),
            "substring" => strings::substring(self, args),
            "string->list" => strings::string_to_list(self, args),
            "list->string" => strings::list_to_string(self, args),
            "string-copy" => strings::string_copy(self, args),

            // Vector operations
            "make-vector" => vectors::make_vector(self, args),
            "vector" => vectors::vector(self, args),
            "vector-length" => vectors::vector_length(self, args),
            "vector-ref" => vectors::vector_ref(self, args),
            "vector-set!" => vectors::vector_set(self, args),
            "vector->list" => vectors::vector_to_list(self, args),
            "list->vector" => vectors::list_to_vector(self, args),
            "vector->string" => vectors::vector_to_string(self, args),
            "string->vector" => vectors::string_to_vector(self, args),
            "vector-copy" => vectors::vector_copy(self, args),
            "vector-copy!" => vectors::vector_copy_bang(self, args),
            "vector-append" => vectors::vector_append(self, args),
            "vector-fill!" => vectors::vector_fill(self, args),
            "vector-map" => vectors::vector_map(self, args),
            "vector-for-each" => vectors::vector_for_each(self, args),

            // Debug primitives
            "debug-enable" => debug::debug_enable(self, args),
            "debug-disable" => debug::debug_disable(self, args),
            "debug-clear" => debug::debug_clear(self, args),
            "debug-status" => debug::debug_status(self, args),
            "debug-mode" => debug::debug_mode(self, args),

            _ => Err(EvalError::InvalidSyntax(format!(
                "Unknown primitive: {}",
                name
            ))),
        }
    }

    /// Install all primitive procedures into the environment
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
            // Debug
            ("debug-enable", Arity::Exact(1)),
            ("debug-disable", Arity::Exact(1)),
            ("debug-clear", Arity::Exact(0)),
            ("debug-status", Arity::Exact(0)),
            ("debug-mode", Arity::Exact(1)),
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
