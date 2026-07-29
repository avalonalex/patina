//! Integer division operations
//!
//! This module implements integer division operations:
//! - quotient, remainder, modulo
//! - floor/, floor-quotient, floor-remainder
//! - truncate/, truncate-quotient, truncate-remainder
//!
//! All operations use Heap methods which have built-in fixnum fast paths.

use super::helpers::numeric_err;
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

/// (quotient n1 n2) - Integer quotient
/// Uses Heap::numeric_quotient with built-in fixnum fast path.
pub(super) fn quotient(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();
    heap_mut
        .numeric_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "quotient"))
}

/// (remainder n1 n2) - Integer remainder
/// Uses Heap::numeric_remainder with built-in fixnum fast path.
pub(super) fn remainder(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();
    heap_mut
        .numeric_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "remainder"))
}

/// (modulo n1 n2) - Integer modulo
/// Uses Heap::numeric_modulo with built-in fixnum fast path.
pub(super) fn modulo(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();
    heap_mut
        .numeric_modulo(args[0], args[1])
        .map_err(|e| numeric_err(e, "modulo"))
}

/// (floor/ n1 n2) -> quotient remainder
/// Returns two values: floor-quotient and floor-remainder.
/// Uses Heap methods with built-in fixnum fast paths.
pub(super) fn floor_div(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();

    let q = heap_mut
        .numeric_floor_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "floor/"))?;
    let r = heap_mut
        .numeric_floor_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "floor/"))?;

    Ok(heap_mut.alloc_values(vec![q, r]))
}

/// (floor-quotient n1 n2) -> quotient
/// Returns floor(n1/n2).
/// Uses Heap::numeric_floor_quotient with built-in fixnum fast path.
pub(super) fn floor_quotient(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();
    heap_mut
        .numeric_floor_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "floor-quotient"))
}

/// (floor-remainder n1 n2) -> remainder
/// Returns n1 - n2 * floor(n1/n2).
/// Uses Heap::numeric_floor_remainder with built-in fixnum fast path.
pub(super) fn floor_remainder(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();
    heap_mut
        .numeric_floor_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "floor-remainder"))
}

/// (truncate/ n1 n2) -> quotient remainder
/// Returns two values: truncate-quotient and truncate-remainder.
/// truncate operations are the same as quotient/remainder (truncation toward zero).
pub(super) fn truncate_div(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();

    // truncate-quotient == quotient, truncate-remainder == remainder
    let q = heap_mut
        .numeric_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "truncate/"))?;
    let r = heap_mut
        .numeric_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "truncate/"))?;

    Ok(heap_mut.alloc_values(vec![q, r]))
}

/// (truncate-quotient n1 n2) -> quotient
/// Returns truncate(n1/n2). Same as quotient (truncation toward zero).
pub(super) fn truncate_quotient(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();
    heap_mut
        .numeric_quotient(args[0], args[1])
        .map_err(|e| numeric_err(e, "truncate-quotient"))
}

/// (truncate-remainder n1 n2) -> remainder
/// Returns n1 - n2 * truncate(n1/n2). Same as remainder.
pub(super) fn truncate_remainder(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }
    let mut heap_mut = heap.borrow_mut();
    heap_mut
        .numeric_remainder(args[0], args[1])
        .map_err(|e| numeric_err(e, "truncate-remainder"))
}
