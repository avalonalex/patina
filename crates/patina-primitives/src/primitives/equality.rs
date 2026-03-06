//! Equality primitive operations (R7RS Section 6.1)
//!
//! Implements R7RS equality predicates with proper semantics:
//! - `eq?` - Reference equality (pointer comparison)
//! - `eqv?` - Value equality (same type, same value)
//! - `equal?` - Structural equality (deep comparison)
//!
//! These operations work directly on TaggedValue using Heap methods.
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

// ========== Equality Primitives ==========

pub(super) fn eq(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(
        heap.borrow().values_eq(args[0], args[1]),
    ))
}

pub(super) fn eqv(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(
        heap.borrow().values_eqv(args[0], args[1]),
    ))
}

pub(super) fn equal(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::boolean(
        heap.borrow().tagged_values_equal(args[0], args[1]),
    ))
}

pub(super) fn register(registry: &mut crate::registry::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // eq? - pointer/symbol equality
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "eq?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are the same object (pointer equality).",
        eq,
    ));

    // eqv? - value equality for numbers, pointer equality otherwise
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "eqv?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are equivalent (value equality for numbers and characters).",
        eqv,
    ));

    // equal? - deep structural equality
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "equal?",
        Arity::Exact(2),
        "Returns #t if obj1 and obj2 are structurally equal (deep comparison).",
        equal,
    ));
}
