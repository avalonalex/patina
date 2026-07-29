//! Rounding and utility operations
//!
//! This module implements:
//! - Rounding: floor, ceiling, truncate, round
//! - Utilities: abs, max, min
//!
//! All operations use Heap methods which have built-in fixnum fast paths.

use super::helpers::numeric_err;
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

/// (floor x) - Rounds x toward negative infinity
/// Uses Heap::numeric_floor with built-in fixnum fast path.
pub(super) fn floor(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_floor(args[0])
        .map_err(|e| numeric_err(e, "floor"))
}

/// (ceiling x) - Rounds x toward positive infinity
/// Uses Heap::numeric_ceiling with built-in fixnum fast path.
pub(super) fn ceiling(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_ceiling(args[0])
        .map_err(|e| numeric_err(e, "ceiling"))
}

/// (truncate x) - Rounds x toward zero
/// Uses Heap::numeric_truncate with built-in fixnum fast path.
pub(super) fn truncate(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_truncate(args[0])
        .map_err(|e| numeric_err(e, "truncate"))
}

/// (round x) - Rounds x to nearest integer (banker's rounding: ties round to even)
/// Uses Heap::numeric_round with built-in fixnum fast path.
pub(super) fn round(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_round(args[0])
        .map_err(|e| numeric_err(e, "round"))
}

/// (abs x) - Absolute value
/// Uses Heap::numeric_abs with built-in fixnum fast path.
pub(super) fn abs(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numeric_abs(args[0])
        .map_err(|e| numeric_err(e, "abs"))
}

/// (max x1 x2 ...) - Maximum of arguments
/// Uses Heap::numeric_max with built-in fixnum fast path.
pub(super) fn max(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    // Single arg case - validate it's a number and return
    if args.len() == 1 {
        return heap
            .borrow_mut()
            .numeric_max(args[0], args[0])
            .map_err(|e| numeric_err(e, "max"));
    }

    // Multi-arg case - fold through arguments
    let mut current_tv = args[0];

    for arg in &args[1..] {
        current_tv = heap
            .borrow_mut()
            .numeric_max(current_tv, *arg)
            .map_err(|e| numeric_err(e, "max"))?;
    }

    Ok(current_tv)
}

/// (min x1 x2 ...) - Minimum of arguments
/// Uses Heap::numeric_min with built-in fixnum fast path.
pub(super) fn min(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    // Single arg case - validate it's a number and return
    if args.len() == 1 {
        return heap
            .borrow_mut()
            .numeric_min(args[0], args[0])
            .map_err(|e| numeric_err(e, "min"));
    }

    // Multi-arg case - fold through arguments
    let mut current_tv = args[0];

    for arg in &args[1..] {
        current_tv = heap
            .borrow_mut()
            .numeric_min(current_tv, *arg)
            .map_err(|e| numeric_err(e, "min"))?;
    }

    Ok(current_tv)
}
