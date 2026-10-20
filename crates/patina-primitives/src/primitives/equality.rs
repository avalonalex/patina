//! Equality primitive operations (R7RS Section 6.1)
//!
//! Implements R7RS equality predicates with proper semantics:
//! - `eq?` - Reference equality (pointer comparison)
//! - `eqv?` - Value equality (same type, same value)
//! - `equal?` - Structural equality (deep comparison)
//!
//! Each takes the context (#706), and asks it (`Cx::values_eq` and the rest).
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

// ========== Equality Primitives ==========

pub(super) fn eq<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::boolean(cx.values_eq(args[0], args[1])))
}

pub(super) fn eqv<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::boolean(cx.values_eqv(args[0], args[1])))
}

pub(super) fn equal<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::boolean(cx.tagged_values_equal(args[0], args[1])))
}

pub(super) fn equal_hash<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    patina_core::census::equal_hash_call();
    let h = cx.tagged_value_hash(args[0]);
    // Return as non-negative fixnum (mod a large prime < fixnum max)
    Ok(Value::fixnum((h % 536870909) as i64))
}

pub(super) fn identity_hash<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let h = cx.tagged_value_hash_identity(args[0]);
    // Same reduction as `equal-hash`: a non-negative fixnum.
    Ok(Value::fixnum((h % 536870909) as i64))
}

pub(super) fn register(registry: &mut crate::registry::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // eq? - pointer/symbol equality
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "eq?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are the same object (pointer equality).",
        eq,
    ));

    // eqv? - value equality for numbers, pointer equality otherwise
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "eqv?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are equivalent (value equality for numbers and characters).",
        eqv,
    ));

    // equal? - deep structural equality
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "equal?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are structurally equal (deep comparison).",
        equal,
    ));

    // equal-hash - hash consistent with equal?
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.predicates",
        "equal-hash",
        Arity::Exact(1),
        "Returns a non-negative exact integer hash code for obj, consistent with equal?.",
        equal_hash,
    ));

    // identity-hash - hash consistent with eq?, for tables keyed by identity
    registry.register(PrimitiveFn::new_cx(
        "patina.internal.predicates",
        "identity-hash",
        Arity::Exact(1),
        "Returns a non-negative exact integer hash code for obj, consistent with eq?.",
        identity_hash,
    ));
}
