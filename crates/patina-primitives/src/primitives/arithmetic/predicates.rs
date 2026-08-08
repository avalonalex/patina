//! Numeric predicates
//!
//! This module implements:
//! - zero?, positive?, negative? - sign predicates
//! - odd?, even? - parity predicates
//! - finite? - Returns #t if x is finite
//! - infinite? - Returns #t if x is infinite
//! - nan? - Returns #t if x is NaN

use super::helpers::numeric_err;
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

fn check_unary(args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(args[0])
}

/// (zero? z) - Returns #t if z is numerically equal to 0.
/// Equivalent to `(= z 0)`, like the former Scheme definition.
pub(super) fn zero_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(TaggedValue::boolean(x.as_fixnum_unchecked() == 0));
    }
    let is_zero = heap
        .borrow()
        .numeric_eq_cmp(x, TaggedValue::fixnum(0))
        .map_err(|e| numeric_err(e, "zero?"))?;
    Ok(TaggedValue::boolean(is_zero))
}

/// (positive? x) - Returns #t if x is greater than 0.
/// Equivalent to `(> x 0)`, like the former Scheme definition.
pub(super) fn positive_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(TaggedValue::boolean(x.as_fixnum_unchecked() > 0));
    }
    let is_pos = heap
        .borrow()
        .numeric_gt(x, TaggedValue::fixnum(0))
        .map_err(|e| numeric_err(e, "positive?"))?;
    Ok(TaggedValue::boolean(is_pos))
}

/// (negative? x) - Returns #t if x is less than 0.
/// Equivalent to `(< x 0)`, like the former Scheme definition.
pub(super) fn negative_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(TaggedValue::boolean(x.as_fixnum_unchecked() < 0));
    }
    let is_neg = heap
        .borrow()
        .numeric_lt(x, TaggedValue::fixnum(0))
        .map_err(|e| numeric_err(e, "negative?"))?;
    Ok(TaggedValue::boolean(is_neg))
}

/// Shared slow path for odd?/even?: `(= (remainder n 2) 0)`, composing the
/// same heap operations as the former Scheme definitions so domain errors
/// (non-integers) are identical.
fn remainder_is_zero(heap: &SharedHeap, x: TaggedValue, name: &str) -> Result<bool, EvalError> {
    let r = heap
        .borrow_mut()
        .numeric_remainder(x, TaggedValue::fixnum(2))
        .map_err(|e| numeric_err(e, name))?;
    heap.borrow()
        .numeric_eq_cmp(r, TaggedValue::fixnum(0))
        .map_err(|e| numeric_err(e, name))
}

/// (odd? n) - Returns #t if the integer n is odd.
pub(super) fn odd_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(TaggedValue::boolean(x.as_fixnum_unchecked() & 1 != 0));
    }
    Ok(TaggedValue::boolean(!remainder_is_zero(heap, x, "odd?")?))
}

/// (even? n) - Returns #t if the integer n is even.
pub(super) fn even_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(TaggedValue::boolean(x.as_fixnum_unchecked() & 1 == 0));
    }
    Ok(TaggedValue::boolean(remainder_is_zero(heap, x, "even?")?))
}

/// (finite? x) - Returns #t if x is finite
pub(super) fn finite_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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
    args: &[TaggedValue],
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
pub(super) fn nan_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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
