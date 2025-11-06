//! Primitive procedures implementation
//!
//! This module implements all built-in Scheme primitive procedures:
//!
//! ## Arithmetic Operations
//! - `+`, `-`, `*`, `/` - Basic arithmetic with overflow detection and inexact contagion
//! - `=`, `<`, `>`, `<=`, `>=` - Numeric comparisons
//! - `quotient`, `remainder`, `modulo` - Integer division operations
//! - `abs`, `max`, `min` - Numeric utilities
//!
//! ## List and Pair Operations
//! - `cons`, `car`, `cdr` - Pair construction and access
//! - `list` - List construction
//! - `length`, `append`, `reverse` - List manipulation
//! - `list-ref`, `list-tail` - List access
//! - `memq`, `memv`, `member` - List membership
//! - `assq`, `assv`, `assoc` - Association list lookup
//!
//! ## Type Predicates
//! - `null?`, `pair?`, `list?` - Type checking
//! - `number?`, `integer?`, `boolean?`, `string?`, `symbol?`
//! - `exact?`, `inexact?` - Exactness predicates
//!
//! ## Equality Operations
//! - `eq?` - Reference equality
//! - `eqv?` - Value equality
//! - `equal?` - Structural equality
//!
//! ## Higher-Order Functions
//! - `map` - Apply function to list elements
//! - `for-each` - Iterate over list elements for side effects
//!
//! ## Multiple Values
//! - `values` - Return multiple values
//! - `call-with-values` - Receive multiple values

use crate::env::Environment;
use crate::value::{Arity, Procedure, Value};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::rc::Rc;

use super::error::EvalError;
use super::Evaluator;

impl Evaluator {
    /// Dispatcher that routes primitive calls to specific implementations
    pub(super) fn apply_primitive(&self, name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
        match name {
            "+" => self.primitive_add(args),
            "-" => self.primitive_subtract(args),
            "*" => self.primitive_multiply(args),
            "/" => self.primitive_divide(args),
            "=" => self.primitive_numeric_equal(args),
            "<" => self.primitive_less_than(args),
            ">" => self.primitive_greater_than(args),
            "<=" => self.primitive_less_equal(args),
            ">=" => self.primitive_greater_equal(args),
            "cons" => self.primitive_cons(args),
            "car" => self.primitive_car(args),
            "cdr" => self.primitive_cdr(args),
            "null?" => self.primitive_null_p(args),
            "pair?" => self.primitive_pair_p(args),
            "list" => Ok(self.list_from_vec(args)),
            "list?" => self.primitive_list_p(args),
            "length" => self.primitive_length(args),
            "append" => self.primitive_append(args),
            "reverse" => self.primitive_reverse(args),
            "list-ref" => self.primitive_list_ref(args),
            "list-tail" => self.primitive_list_tail(args),
            "memq" => self.primitive_memq(args),
            "memv" => self.primitive_memv(args),
            "member" => self.primitive_member(args),
            "assq" => self.primitive_assq(args),
            "assv" => self.primitive_assv(args),
            "assoc" => self.primitive_assoc(args),
            "map" => self.primitive_map(args),
            "for-each" => self.primitive_for_each(args),
            "number?" => self.primitive_number_p(args),
            "integer?" => self.primitive_integer_p(args),
            "boolean?" => self.primitive_boolean_p(args),
            "string?" => self.primitive_string_p(args),
            "symbol?" => self.primitive_symbol_p(args),
            "eq?" => self.primitive_eq(args),
            "eqv?" => self.primitive_eqv(args),
            "equal?" => self.primitive_equal(args),
            "values" => self.primitive_values(args),
            "call-with-values" => self.primitive_call_with_values(args),
            "quotient" => self.primitive_quotient(args),
            "remainder" => self.primitive_remainder(args),
            "modulo" => self.primitive_modulo(args),
            "abs" => self.primitive_abs(args),
            "max" => self.primitive_max(args),
            "min" => self.primitive_min(args),
            "exact?" => self.primitive_exact_p(args),
            "inexact?" => self.primitive_inexact_p(args),
            _ => Err(EvalError::InvalidSyntax(format!(
                "Unknown primitive: {}",
                name
            ))),
        }
    }

    /// Install all primitives into the global environment
    pub(super) fn install_primitives(env: &Rc<Environment>) {
        let primitives = [
            ("+", Arity::Min(0)),
            ("-", Arity::Min(1)),
            ("*", Arity::Min(0)),
            ("/", Arity::Min(1)),
            ("=", Arity::Min(2)),
            ("<", Arity::Min(2)),
            (">", Arity::Min(2)),
            ("<=", Arity::Min(2)),
            (">=", Arity::Min(2)),
            ("cons", Arity::Exact(2)),
            ("car", Arity::Exact(1)),
            ("cdr", Arity::Exact(1)),
            ("null?", Arity::Exact(1)),
            ("pair?", Arity::Exact(1)),
            ("list", Arity::Min(0)),
            ("list?", Arity::Exact(1)),
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
            ("map", Arity::Min(2)),
            ("for-each", Arity::Min(2)),
            ("number?", Arity::Exact(1)),
            ("integer?", Arity::Exact(1)),
            ("boolean?", Arity::Exact(1)),
            ("string?", Arity::Exact(1)),
            ("symbol?", Arity::Exact(1)),
            ("eq?", Arity::Exact(2)),
            ("eqv?", Arity::Exact(2)),
            ("equal?", Arity::Exact(2)),
            ("values", Arity::Min(0)),
            ("call-with-values", Arity::Exact(2)),
            ("quotient", Arity::Exact(2)),
            ("remainder", Arity::Exact(2)),
            ("modulo", Arity::Exact(2)),
            ("abs", Arity::Exact(1)),
            ("max", Arity::Min(1)),
            ("min", Arity::Min(1)),
            ("exact?", Arity::Exact(1)),
            ("inexact?", Arity::Exact(1)),
        ];

        for (name, arity) in primitives {
            env.define(
                name.to_string(),
                Value::Procedure(Procedure::Primitive { name, arity }),
            );
        }
    }

    // ===== Arithmetic Primitives =====

    pub(super) fn primitive_add(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut result: Value = Value::Integer(0);

        for arg in args {
            result = match (result, arg) {
                (Value::Integer(a), Value::Integer(b)) => match a.checked_add(b) {
                    Some(sum) => Value::Integer(sum),
                    None => Value::BigInteger(BigInt::from(a) + BigInt::from(b)),
                },
                (Value::BigInteger(a), Value::Integer(b)) => Value::BigInteger(a + BigInt::from(b)),
                (Value::Integer(a), Value::BigInteger(b)) => Value::BigInteger(BigInt::from(a) + b),
                (Value::BigInteger(a), Value::BigInteger(b)) => Value::BigInteger(a + b),
                (Value::Integer(a), Value::Real(b)) => Value::Real(a as f64 + b),
                (Value::Real(a), Value::Integer(b)) => Value::Real(a + b as f64),
                (Value::BigInteger(a), Value::Real(b)) => {
                    Value::Real(a.to_f64().unwrap_or(f64::INFINITY) + b)
                }
                (Value::Real(a), Value::BigInteger(b)) => {
                    Value::Real(a + b.to_f64().unwrap_or(f64::INFINITY))
                }
                (Value::Real(a), Value::Real(b)) => Value::Real(a + b),
                (_, other) => {
                    return Err(EvalError::TypeError(format!(
                        "+ expects numbers, got {}",
                        other.type_name()
                    )))
                }
            };
        }
        Ok(result)
    }

    pub(super) fn primitive_subtract(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        if args.len() == 1 {
            return match &args[0] {
                Value::Integer(n) => match n.checked_neg() {
                    Some(neg) => Ok(Value::Integer(neg)),
                    None => Ok(Value::BigInteger(-BigInt::from(*n))),
                },
                Value::BigInteger(n) => Ok(Value::BigInteger(-n)),
                Value::Real(n) => Ok(Value::Real(-n)),
                other => Err(EvalError::TypeError(format!(
                    "- expects numbers, got {}",
                    other.type_name()
                ))),
            };
        }

        let mut result = args[0].clone();

        for arg in &args[1..] {
            result = match (result, arg) {
                (Value::Integer(a), Value::Integer(b)) => match a.checked_sub(*b) {
                    Some(diff) => Value::Integer(diff),
                    None => Value::BigInteger(BigInt::from(a) - BigInt::from(*b)),
                },
                (Value::BigInteger(a), Value::Integer(b)) => {
                    Value::BigInteger(a - BigInt::from(*b))
                }
                (Value::Integer(a), Value::BigInteger(b)) => Value::BigInteger(BigInt::from(a) - b),
                (Value::BigInteger(a), Value::BigInteger(b)) => Value::BigInteger(a - b),
                (Value::Integer(a), Value::Real(b)) => Value::Real(a as f64 - *b),
                (Value::Real(a), Value::Integer(b)) => Value::Real(a - *b as f64),
                (Value::BigInteger(a), Value::Real(b)) => {
                    Value::Real(a.to_f64().unwrap_or(f64::INFINITY) - *b)
                }
                (Value::Real(a), Value::BigInteger(b)) => {
                    Value::Real(a - b.to_f64().unwrap_or(f64::INFINITY))
                }
                (Value::Real(a), Value::Real(b)) => Value::Real(a - *b),
                (_, other) => {
                    return Err(EvalError::TypeError(format!(
                        "- expects numbers, got {}",
                        other.type_name()
                    )))
                }
            };
        }
        Ok(result)
    }

    pub(super) fn primitive_multiply(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut result: Value = Value::Integer(1);

        for arg in args {
            result = match (result, arg) {
                (Value::Integer(a), Value::Integer(b)) => match a.checked_mul(b) {
                    Some(product) => Value::Integer(product),
                    None => Value::BigInteger(BigInt::from(a) * BigInt::from(b)),
                },
                (Value::BigInteger(a), Value::Integer(b)) => Value::BigInteger(a * BigInt::from(b)),
                (Value::Integer(a), Value::BigInteger(b)) => Value::BigInteger(BigInt::from(a) * b),
                (Value::BigInteger(a), Value::BigInteger(b)) => Value::BigInteger(a * b),
                (Value::Integer(a), Value::Real(b)) => Value::Real(a as f64 * b),
                (Value::Real(a), Value::Integer(b)) => Value::Real(a * b as f64),
                (Value::BigInteger(a), Value::Real(b)) => {
                    Value::Real(a.to_f64().unwrap_or(f64::INFINITY) * b)
                }
                (Value::Real(a), Value::BigInteger(b)) => {
                    Value::Real(a * b.to_f64().unwrap_or(f64::INFINITY))
                }
                (Value::Real(a), Value::Real(b)) => Value::Real(a * b),
                (_, other) => {
                    return Err(EvalError::TypeError(format!(
                        "* expects numbers, got {}",
                        other.type_name()
                    )))
                }
            };
        }
        Ok(result)
    }

    pub(super) fn primitive_divide(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let to_f64 = |v: &Value| -> Result<f64, EvalError> {
            match v {
                Value::Integer(n) => Ok(*n as f64),
                Value::BigInteger(n) => Ok(n.to_f64().unwrap_or(f64::INFINITY)),
                Value::Real(n) => Ok(*n),
                other => Err(EvalError::TypeError(format!(
                    "/ expects numbers, got {}",
                    other.type_name()
                ))),
            }
        };

        if args.len() == 1 {
            let x = to_f64(&args[0])?;
            if x == 0.0 {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(Value::Real(1.0 / x));
        }

        let mut result = to_f64(&args[0])?;

        for arg in &args[1..] {
            let divisor = to_f64(arg)?;
            if divisor == 0.0 {
                return Err(EvalError::DivisionByZero);
            }
            result /= divisor;
        }

        Ok(Value::Real(result))
    }

    // ===== Comparison Primitives =====

    pub(super) fn value_to_f64(&self, v: &Value) -> Result<f64, EvalError> {
        match v {
            Value::Integer(n) => Ok(*n as f64),
            Value::BigInteger(n) => Ok(n.to_f64().unwrap_or(f64::INFINITY)),
            Value::Real(n) => Ok(*n),
            other => Err(EvalError::TypeError(format!(
                "Expected number, got {}",
                other.type_name()
            ))),
        }
    }

    pub(super) fn primitive_numeric_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        let first = self.value_to_f64(&args[0])?;
        for arg in &args[1..] {
            let n = self.value_to_f64(arg)?;
            if (first - n).abs() > f64::EPSILON {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }

    pub(super) fn primitive_less_than(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        for i in 0..args.len() - 1 {
            let a = self.value_to_f64(&args[i])?;
            let b = self.value_to_f64(&args[i + 1])?;
            if a >= b {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }

    pub(super) fn primitive_greater_than(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        for i in 0..args.len() - 1 {
            let a = self.value_to_f64(&args[i])?;
            let b = self.value_to_f64(&args[i + 1])?;
            if a <= b {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }

    pub(super) fn primitive_less_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        for i in 0..args.len() - 1 {
            let a = self.value_to_f64(&args[i])?;
            let b = self.value_to_f64(&args[i + 1])?;
            if a > b {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }

    pub(super) fn primitive_greater_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        for i in 0..args.len() - 1 {
            let a = self.value_to_f64(&args[i])?;
            let b = self.value_to_f64(&args[i + 1])?;
            if a < b {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }

    // ===== Pair/List Primitives =====

    pub(super) fn primitive_cons(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Pair(Rc::new((args[0].clone(), args[1].clone()))))
    }

    pub(super) fn primitive_car(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Pair(pair) => Ok(pair.0.clone()),
            _ => Err(EvalError::TypeError("car expects a pair".to_string())),
        }
    }

    pub(super) fn primitive_cdr(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Pair(pair) => Ok(pair.1.clone()),
            _ => Err(EvalError::TypeError("cdr expects a pair".to_string())),
        }
    }

    pub(super) fn primitive_list_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let mut slow = args[0].clone();
        let mut fast = args[0].clone();

        loop {
            match &fast {
                Value::Null => return Ok(Value::Boolean(true)),
                Value::Pair(pair1) => {
                    fast = pair1.1.clone();
                    match &fast {
                        Value::Null => return Ok(Value::Boolean(true)),
                        Value::Pair(pair2) => {
                            fast = pair2.1.clone();
                            if let Value::Pair(slow_pair) = &slow {
                                slow = slow_pair.1.clone();
                            }
                            if Self::values_equal(&slow, &fast)? {
                                return Ok(Value::Boolean(false));
                            }
                        }
                        _ => return Ok(Value::Boolean(false)),
                    }
                }
                _ => return Ok(Value::Boolean(false)),
            }
        }
    }

    pub(super) fn primitive_length(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let mut count = 0;
        let mut current = args[0].clone();

        while let Value::Pair(pair) = current {
            count += 1;
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(
                "length: argument must be a proper list".to_string(),
            ));
        }

        Ok(Value::Integer(count))
    }

    pub(super) fn primitive_append(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Ok(Value::Null);
        }

        if args.len() == 1 {
            return Ok(args[0].clone());
        }

        let mut result_items = Vec::new();
        for (i, list) in args.iter().enumerate() {
            if i == args.len() - 1 {
                if result_items.is_empty() {
                    return Ok(list.clone());
                }
                let mut result = list.clone();
                for item in result_items.into_iter().rev() {
                    result = Value::Pair(Rc::new((item, result)));
                }
                return Ok(result);
            }

            let mut current = list.clone();
            while let Value::Pair(pair) = current {
                result_items.push(pair.0.clone());
                current = pair.1.clone();
            }

            if !matches!(current, Value::Null) {
                return Err(EvalError::TypeError(format!(
                    "append: argument {} must be a proper list",
                    i + 1
                )));
            }
        }

        Ok(self.list_from_vec(result_items))
    }

    pub(super) fn primitive_reverse(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let mut items = Vec::new();
        let mut current = args[0].clone();

        while let Value::Pair(pair) = current {
            items.push(pair.0.clone());
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(
                "reverse: argument must be a proper list".to_string(),
            ));
        }

        items.reverse();
        Ok(self.list_from_vec(items))
    }

    pub(super) fn primitive_list_ref(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let k = match &args[1] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "list-ref: index must be non-negative".to_string(),
                ))
            }
            _ => {
                return Err(EvalError::TypeError(
                    "list-ref: index must be an integer".to_string(),
                ))
            }
        };

        let mut current = args[0].clone();
        let mut index = 0;

        while let Value::Pair(pair) = current {
            if index == k {
                return Ok(pair.0.clone());
            }
            index += 1;
            current = pair.1.clone();
        }

        Err(EvalError::TypeError(
            "list-ref: index out of bounds".to_string(),
        ))
    }

    pub(super) fn primitive_list_tail(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let k = match &args[1] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "list-tail: index must be non-negative".to_string(),
                ))
            }
            _ => {
                return Err(EvalError::TypeError(
                    "list-tail: index must be an integer".to_string(),
                ))
            }
        };

        let mut current = args[0].clone();
        let mut index = 0;

        while index < k {
            match current {
                Value::Pair(pair) => {
                    current = pair.1.clone();
                    index += 1;
                }
                _ => {
                    return Err(EvalError::TypeError(
                        "list-tail: index out of bounds".to_string(),
                    ))
                }
            }
        }

        Ok(current)
    }

    // ===== List Search Primitives =====

    pub(super) fn primitive_memq(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();

        while let Value::Pair(pair) = current {
            if self.values_eq(obj, &pair.0) {
                return Ok(Value::Pair(pair));
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_memv(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();

        while let Value::Pair(pair) = current {
            if self.values_eqv(obj, &pair.0) {
                return Ok(Value::Pair(pair));
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_member(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 || args.len() > 3 {
            return Err(EvalError::WrongArity {
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();
        let compare_proc = args.get(2);

        while let Value::Pair(pair) = current.clone() {
            let matches = if let Some(proc) = compare_proc {
                let result = self.apply(proc.clone(), vec![obj.clone(), pair.0.clone()])?;
                result.is_truthy()
            } else {
                Self::values_equal(obj, &pair.0)?
            };

            if matches {
                return Ok(Value::Pair(pair));
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_assq(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();

        while let Value::Pair(pair) = current {
            if let Value::Pair(entry) = &pair.0 {
                if self.values_eq(obj, &entry.0) {
                    return Ok(pair.0.clone());
                }
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_assv(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();

        while let Value::Pair(pair) = current {
            if let Value::Pair(entry) = &pair.0 {
                if self.values_eqv(obj, &entry.0) {
                    return Ok(pair.0.clone());
                }
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_assoc(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 || args.len() > 3 {
            return Err(EvalError::WrongArity {
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();
        let compare_proc = args.get(2);

        while let Value::Pair(pair) = current.clone() {
            if let Value::Pair(entry) = &pair.0 {
                let matches = if let Some(proc) = compare_proc {
                    let result = self.apply(proc.clone(), vec![obj.clone(), entry.0.clone()])?;
                    result.is_truthy()
                } else {
                    Self::values_equal(obj, &entry.0)?
                };

                if matches {
                    return Ok(pair.0.clone());
                }
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    // ===== Type Predicates =====

    pub(super) fn primitive_null_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Null)))
    }

    pub(super) fn primitive_pair_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Pair(_))))
    }

    pub(super) fn primitive_number_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(
            args[0],
            Value::Integer(_)
                | Value::BigInteger(_)
                | Value::Rational(_)
                | Value::Real(_)
                | Value::Complex(_, _)
        )))
    }

    pub(super) fn primitive_integer_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(
            args[0],
            Value::Integer(_) | Value::BigInteger(_)
        )))
    }

    pub(super) fn primitive_boolean_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Boolean(_))))
    }

    pub(super) fn primitive_string_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::String(_))))
    }

    pub(super) fn primitive_symbol_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Symbol(_))))
    }

    pub(super) fn primitive_exact_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(
            args[0],
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_)
        )))
    }

    pub(super) fn primitive_inexact_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(
            args[0],
            Value::Real(_) | Value::Complex(_, _)
        )))
    }

    // ===== Equality Primitives =====

    pub(super) fn values_eq(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Symbol(a), Value::Symbol(b)) => a.as_ref() == b.as_ref(),
            _ => false,
        }
    }

    pub(super) fn primitive_eq(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Ok(Value::Boolean(self.values_eq(&args[0], &args[1])))
    }

    pub(super) fn values_eqv(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Symbol(a), Value::Symbol(b)) => a.as_ref() == b.as_ref(),
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a == b,
            (Value::Character(a), Value::Character(b)) => a == b,
            _ => false,
        }
    }

    pub(super) fn primitive_eqv(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Ok(Value::Boolean(self.values_eqv(&args[0], &args[1])))
    }

    pub(super) fn primitive_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Ok(Value::Boolean(Self::values_equal(&args[0], &args[1])?))
    }

    pub(super) fn values_equal(a: &Value, b: &Value) -> Result<bool, EvalError> {
        Ok(match (a, b) {
            (Value::Boolean(x), Value::Boolean(y)) => x == y,
            (Value::Integer(x), Value::Integer(y)) => x == y,
            (Value::Real(x), Value::Real(y)) => x == y,
            (Value::Character(x), Value::Character(y)) => x == y,
            (Value::Null, Value::Null) => true,
            (Value::String(x), Value::String(y)) => x.as_ref() == y.as_ref(),
            (Value::Symbol(x), Value::Symbol(y)) => x.as_ref() == y.as_ref(),
            (Value::Pair(x), Value::Pair(y)) => {
                Self::values_equal(&x.0, &y.0)? && Self::values_equal(&x.1, &y.1)?
            }
            (Value::Vector(x), Value::Vector(y)) => {
                if x.len() != y.len() {
                    false
                } else {
                    let mut equal = true;
                    for (a, b) in x.iter().zip(y.iter()) {
                        if !Self::values_equal(a, b)? {
                            equal = false;
                            break;
                        }
                    }
                    equal
                }
            }
            _ => false,
        })
    }

    // ===== Higher-Order Functions =====

    pub(super) fn primitive_map(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let proc = &args[0];
        let lists = &args[1..];

        if lists.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: 1,
            });
        }

        let mut list_vecs: Vec<Vec<Value>> = Vec::new();
        for list in lists {
            let mut items = Vec::new();
            let mut current = list.clone();

            while let Value::Pair(pair) = current {
                items.push(pair.0.clone());
                current = pair.1.clone();
            }

            if !matches!(current, Value::Null) {
                return Err(EvalError::TypeError(
                    "map: argument must be a proper list".to_string(),
                ));
            }

            list_vecs.push(items);
        }

        let min_len = list_vecs.iter().map(|v| v.len()).min().unwrap_or(0);

        let mut results = Vec::new();
        for i in 0..min_len {
            let mut proc_args = Vec::new();
            for list_vec in &list_vecs {
                proc_args.push(list_vec[i].clone());
            }

            let result = self.apply(proc.clone(), proc_args)?;
            results.push(result);
        }

        Ok(self.list_from_vec(results))
    }

    pub(super) fn primitive_for_each(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let proc = &args[0];
        let lists = &args[1..];

        if lists.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: 1,
            });
        }

        let mut list_vecs: Vec<Vec<Value>> = Vec::new();
        for list in lists {
            let mut items = Vec::new();
            let mut current = list.clone();

            while let Value::Pair(pair) = current {
                items.push(pair.0.clone());
                current = pair.1.clone();
            }

            if !matches!(current, Value::Null) {
                return Err(EvalError::TypeError(
                    "for-each: argument must be a proper list".to_string(),
                ));
            }

            list_vecs.push(items);
        }

        let min_len = list_vecs.iter().map(|v| v.len()).min().unwrap_or(0);

        for i in 0..min_len {
            let mut proc_args = Vec::new();
            for list_vec in &list_vecs {
                proc_args.push(list_vec[i].clone());
            }

            self.apply(proc.clone(), proc_args)?;
        }

        Ok(Value::Unspecified)
    }

    // ===== Multiple Values Primitives =====

    pub(super) fn primitive_values(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        match args.len() {
            0 => Ok(Value::Unspecified),
            1 => Ok(args.into_iter().next().unwrap()),
            _ => Ok(Value::Values(args)),
        }
    }

    pub(super) fn primitive_call_with_values(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let producer = &args[0];
        let consumer = &args[1];

        let produced = self.apply(producer.clone(), vec![])?;

        let consumer_args = match produced {
            Value::Values(vals) => vals,
            other => vec![other],
        };

        self.apply(consumer.clone(), consumer_args)
    }

    // ===== Numeric Operations =====

    pub(super) fn primitive_quotient(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        match (&args[0], &args[1]) {
            (Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::Integer(a / b))
            }
            (Value::BigInteger(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(a / b))
            }
            (Value::Integer(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(BigInt::from(*a) / b))
            }
            (Value::BigInteger(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(a / BigInt::from(*b)))
            }
            _ => Err(EvalError::TypeError(format!(
                "quotient requires integers, got {} and {}",
                args[0].type_name(),
                args[1].type_name()
            ))),
        }
    }

    pub(super) fn primitive_remainder(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        match (&args[0], &args[1]) {
            (Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::Integer(a % b))
            }
            (Value::BigInteger(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(a % b))
            }
            (Value::Integer(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(BigInt::from(*a) % b))
            }
            (Value::BigInteger(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(a % BigInt::from(*b)))
            }
            _ => Err(EvalError::TypeError(format!(
                "remainder requires integers, got {} and {}",
                args[0].type_name(),
                args[1].type_name()
            ))),
        }
    }

    pub(super) fn primitive_modulo(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        match (&args[0], &args[1]) {
            (Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::Integer(a.rem_euclid(*b)))
            }
            (Value::BigInteger(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                use num_traits::Euclid;
                Ok(Value::BigInteger(a.rem_euclid(b)))
            }
            (Value::Integer(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                use num_traits::Euclid;
                Ok(Value::BigInteger(BigInt::from(*a).rem_euclid(b)))
            }
            (Value::BigInteger(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                use num_traits::Euclid;
                Ok(Value::BigInteger(a.rem_euclid(&BigInt::from(*b))))
            }
            _ => Err(EvalError::TypeError(format!(
                "modulo requires integers, got {} and {}",
                args[0].type_name(),
                args[1].type_name()
            ))),
        }
    }

    pub(super) fn primitive_abs(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Integer(n) => {
                if *n == i64::MIN {
                    Ok(Value::BigInteger(BigInt::from(*n).abs()))
                } else {
                    Ok(Value::Integer(n.abs()))
                }
            }
            Value::BigInteger(n) => Ok(Value::BigInteger(n.abs())),
            Value::Rational(r) => Ok(Value::Rational(r.abs())),
            Value::Real(r) => Ok(Value::Real(r.abs())),
            _ => Err(EvalError::TypeError(format!(
                "abs requires a number, got {}",
                args[0].type_name()
            ))),
        }
    }

    pub(super) fn primitive_max(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let mut max_val = match &args[0] {
            Value::Integer(n) => *n,
            _ => {
                return Err(EvalError::TypeError(format!(
                    "max requires integers, got {}",
                    args[0].type_name()
                )))
            }
        };

        for arg in &args[1..] {
            match arg {
                Value::Integer(n) => {
                    if *n > max_val {
                        max_val = *n;
                    }
                }
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "max requires integers, got {}",
                        arg.type_name()
                    )))
                }
            }
        }

        Ok(Value::Integer(max_val))
    }

    pub(super) fn primitive_min(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let mut min_val = match &args[0] {
            Value::Integer(n) => *n,
            _ => {
                return Err(EvalError::TypeError(format!(
                    "min requires integers, got {}",
                    args[0].type_name()
                )))
            }
        };

        for arg in &args[1..] {
            match arg {
                Value::Integer(n) => {
                    if *n < min_val {
                        min_val = *n;
                    }
                }
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "min requires integers, got {}",
                        arg.type_name()
                    )))
                }
            }
        }

        Ok(Value::Integer(min_val))
    }
}
