//! Equality primitive operations (R7RS Section 6.1)
//!
//! Implements R7RS equality predicates with proper semantics:
//! - `eq?` - Reference equality (pointer comparison)
//! - `eqv?` - Value equality (same type, same value)
//! - `equal?` - Structural equality (deep comparison)
//!
//! INSTRUCTIONS: Move from primitives.rs:
//! - primitive_eq()
//! - primitive_eqv()
//! - primitive_equal()
//! - values_eq() helper
//! - values_eqv() helper
//! - values_equal() helper

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_runtime::value::Value;

pub(super) fn eq(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "eq?")?;
    Ok(Value::Boolean(values_eq(&args[0], &args[1])))
}

pub(super) fn eqv(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "eqv?")?;
    Ok(Value::Boolean(values_eqv(&args[0], &args[1])))
}

pub(super) fn equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "equal?")?;
    Ok(Value::Boolean(values_equal(&args[0], &args[1])?))
}

// Helper functions (used by eq/eqv/equal and other modules)

pub(in crate::eval) fn values_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        (Value::Null, Value::Null) => true,
        (Value::Symbol(a), Value::Symbol(b)) => a.as_ref() == b.as_ref(),
        _ => false,
    }
}

pub(in crate::eval) fn values_eqv(a: &Value, b: &Value) -> bool {
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

pub(in crate::eval) fn values_equal(a: &Value, b: &Value) -> Result<bool, EvalError> {
    Ok(match (a, b) {
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::Integer(x), Value::Integer(y)) => x == y,
        (Value::BigInteger(x), Value::BigInteger(y)) => x == y,
        (Value::Rational(x), Value::Rational(y)) => x == y,
        (Value::Real(x), Value::Real(y)) => x == y, // IEEE 754 exact equality
        (Value::Complex(r1, i1), Value::Complex(r2, i2)) => r1 == r2 && i1 == i2, // IEEE 754 exact equality
        (Value::Character(x), Value::Character(y)) => x == y,
        (Value::Null, Value::Null) => true,
        (Value::String(x), Value::String(y)) => *x.borrow() == *y.borrow(),
        (Value::Symbol(x), Value::Symbol(y)) => x.as_ref() == y.as_ref(),
        (Value::Pair(x), Value::Pair(y)) => values_equal(&x.0, &y.0)? && values_equal(&x.1, &y.1)?,
        (Value::Vector(x), Value::Vector(y)) => {
            let x_vec = x.borrow();
            let y_vec = y.borrow();
            if x_vec.len() != y_vec.len() {
                false
            } else {
                let mut equal = true;
                for (a, b) in x_vec.iter().zip(y_vec.iter()) {
                    if !values_equal(a, b)? {
                        equal = false;
                        break;
                    }
                }
                equal
            }
        }
        (Value::Bytevector(x), Value::Bytevector(y)) => x == y,
        _ => false,
    })
}
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // eq? - pointer/symbol equality
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "eq?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are the same object (pointer equality).",
        |eval, args, _tail| eq(eval, args).map(EvalResult::Value),
    ));

    // eqv? - value equality for numbers, pointer equality otherwise
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "eqv?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are equivalent (value equality for numbers and characters).",
        |eval, args, _tail| eqv(eval, args).map(EvalResult::Value),
    ));

    // equal? - deep structural equality
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "equal?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are structurally equal (deep comparison).",
        |eval, args, _tail| equal(eval, args).map(EvalResult::Value),
    ));
}
