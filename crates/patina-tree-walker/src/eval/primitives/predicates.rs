//! Type predicate primitives (R7RS Section 6.x)
//!
//! Implements type checking predicates for all Scheme types.
//!
//! INSTRUCTIONS: Move from primitives.rs:
//! - primitive_number_p()
//! - primitive_integer_p()
//! - primitive_boolean_p()
//! - primitive_string_p()
//! - primitive_symbol_p()
//! - primitive_null_p()
//! - primitive_pair_p()
//! - primitive_list_p()
//! - primitive_exact_p()
//! - primitive_inexact_p()
//! - primitive_boolean_equal()
//! - primitive_procedure_p()
//! - primitive_char_p()
//! - primitive_vector_p()

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_runtime::value::Value;

pub(super) fn number_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| {
        matches!(
            v,
            Value::Integer(_)
                | Value::BigInteger(_)
                | Value::Rational(_)
                | Value::Real(_)
                | Value::Complex(_, _)
        )
    })
}

pub(super) fn integer_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| {
        matches!(v, Value::Integer(_) | Value::BigInteger(_))
    })
}

pub(super) fn rational_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| {
        matches!(
            v,
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_)
        )
    })
}

pub(super) fn real_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| {
        matches!(
            v,
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) | Value::Real(_)
        )
    })
}

pub(super) fn complex_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| {
        matches!(
            v,
            Value::Integer(_)
                | Value::BigInteger(_)
                | Value::Rational(_)
                | Value::Real(_)
                | Value::Complex(_, _)
        )
    })
}

pub(super) fn boolean_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Boolean(_)))
}

pub(super) fn string_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::String(_)))
}

pub(super) fn symbol_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Symbol(_)))
}

pub(super) fn null_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Null))
}

pub(super) fn pair_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Pair(_)))
}

pub(super) fn list_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "list?")?;

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
                        if super::equality::values_equal(&slow, &fast)? {
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

pub(super) fn exact_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| {
        matches!(
            v,
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_)
        )
    })
}

pub(super) fn inexact_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Real(_) | Value::Complex(_, _)))
}

pub(super) fn boolean_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "boolean=?")?;

    // First, check that all arguments are booleans
    for arg in &args {
        if !matches!(arg, Value::Boolean(_)) {
            return Err(EvalError::TypeError(
                "boolean=? expects all arguments to be booleans".to_string(),
            ));
        }
    }

    // Extract first boolean value
    let first_val = match &args[0] {
        Value::Boolean(b) => *b,
        _ => unreachable!(), // Already checked above
    };

    // Check that all remaining values are equal to the first
    for arg in &args[1..] {
        match arg {
            Value::Boolean(b) => {
                if *b != first_val {
                    return Ok(Value::Boolean(false));
                }
            }
            _ => unreachable!(), // Already checked above
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn procedure_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Procedure(_)))
}

pub(super) fn char_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Character(_)))
}

pub(super) fn vector_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Vector(_)))
}

pub(super) fn exact_integer_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| match v {
        Value::Integer(_) | Value::BigInteger(_) => true,
        Value::Rational(r) => {
            // A rational is an exact integer if its denominator is 1
            use num_bigint::BigInt;
            r.denom() == &BigInt::from(1)
        }
        _ => false,
    })
}

pub(super) fn library_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Library(_)))
}
