//! Float predicates
//!
//! This module implements:
//! - finite? - Returns #t if x is finite
//! - infinite? - Returns #t if x is infinite
//! - nan? - Returns #t if x is NaN

use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

/// (finite? x) - Returns #t if x is finite
pub(super) fn finite_p(
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();

    match heap_ref.numeric_is_finite(args[0]) {
        Some(is_finite) => Ok(TaggedValue::boolean(is_finite)),
        None => Err(EvalError::TypeError(format!(
            "finite? expects a number, got {}",
            heap_ref.type_name(args[0])
        ))),
    }
}

/// (infinite? x) - Returns #t if x is infinite
pub(super) fn infinite_p(
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();

    match heap_ref.numeric_is_infinite(args[0]) {
        Some(is_infinite) => Ok(TaggedValue::boolean(is_infinite)),
        None => Err(EvalError::TypeError(format!(
            "infinite? expects a number, got {}",
            heap_ref.type_name(args[0])
        ))),
    }
}

/// (nan? x) - Returns #t if x is NaN
pub(super) fn nan_p(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();

    match heap_ref.numeric_is_nan(args[0]) {
        Some(is_nan) => Ok(TaggedValue::boolean(is_nan)),
        None => Err(EvalError::TypeError(format!(
            "nan? expects a number, got {}",
            heap_ref.type_name(args[0])
        ))),
    }
}
