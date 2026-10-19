//! Numeric predicates
//!
//! This module implements:
//! - zero?, positive?, negative? - sign predicates
//! - odd?, even? - parity predicates
//! - finite? - Returns #t if x is finite
//! - infinite? - Returns #t if x is infinite
//! - nan? - Returns #t if x is NaN
//!
//! The sign/parity predicates deliberately keep their own bodies rather than
//! delegating to the `=`/`<`/`>`/`remainder` handlers: they call the same
//! heap ops (which own the numeric-tower semantics), but label domain errors
//! with the predicate's own name — `(zero? "a")` says `zero?`, not `=`.

use super::helpers::numeric_err;
use crate::registry::expect_arity;
use patina_core::{Cx, Value};
use patina_runtime::EvalError;

fn check_unary<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    expect_arity(args, 1)?;
    Ok(args[0])
}

/// (zero? z) - Returns #t if z is numerically equal to 0.
/// Equivalent to `(= z 0)`, like the former Scheme definition.
pub(super) fn zero_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(Value::boolean(x.as_fixnum_unchecked() == 0));
    }
    let is_zero = cx
        .numeric_eq_cmp(x, Value::fixnum(0))
        .map_err(|e| numeric_err(e, "zero?"))?;
    Ok(Value::boolean(is_zero))
}

/// (positive? x) - Returns #t if x is greater than 0.
/// Equivalent to `(> x 0)`, like the former Scheme definition.
pub(super) fn positive_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(Value::boolean(x.as_fixnum_unchecked() > 0));
    }
    let is_pos = cx
        .numeric_gt(x, Value::fixnum(0))
        .map_err(|e| numeric_err(e, "positive?"))?;
    Ok(Value::boolean(is_pos))
}

/// (negative? x) - Returns #t if x is less than 0.
/// Equivalent to `(< x 0)`, like the former Scheme definition.
pub(super) fn negative_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(Value::boolean(x.as_fixnum_unchecked() < 0));
    }
    let is_neg = cx
        .numeric_lt(x, Value::fixnum(0))
        .map_err(|e| numeric_err(e, "negative?"))?;
    Ok(Value::boolean(is_neg))
}

/// Shared slow path for odd?/even?: `(= (remainder n 2) 0)`, composing the
/// same heap operations as the former Scheme definitions so domain errors
/// (non-integers) are identical.
fn remainder_is_zero<'gc>(cx: &mut Cx<'gc>, x: Value<'gc>, name: &str) -> Result<bool, EvalError> {
    let r = cx
        .numeric_remainder(x, Value::fixnum(2))
        .map_err(|e| numeric_err(e, name))?;
    cx.numeric_eq_cmp(r, Value::fixnum(0))
        .map_err(|e| numeric_err(e, name))
}

/// (odd? n) - Returns #t if the integer n is odd.
pub(super) fn odd_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(Value::boolean(x.as_fixnum_unchecked() & 1 != 0));
    }
    Ok(Value::boolean(!remainder_is_zero(cx, x, "odd?")?))
}

/// (even? n) - Returns #t if the integer n is even.
pub(super) fn even_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let x = check_unary(args)?;
    if x.is_fixnum() {
        return Ok(Value::boolean(x.as_fixnum_unchecked() & 1 == 0));
    }
    Ok(Value::boolean(remainder_is_zero(cx, x, "even?")?))
}

/// (finite? x) - Returns #t if x is finite
pub(super) fn finite_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    check_unary(args)?;

    match cx.numeric_is_finite(args[0]) {
        Some(is_finite) => Ok(Value::boolean(is_finite)),
        None => Err(EvalError::TypeError(format!(
            "finite? expects a number, got {}",
            cx.type_name(args[0])
        ))),
    }
}

/// (infinite? x) - Returns #t if x is infinite
pub(super) fn infinite_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    check_unary(args)?;

    match cx.numeric_is_infinite(args[0]) {
        Some(is_infinite) => Ok(Value::boolean(is_infinite)),
        None => Err(EvalError::TypeError(format!(
            "infinite? expects a number, got {}",
            cx.type_name(args[0])
        ))),
    }
}

/// (nan? x) - Returns #t if x is NaN
pub(super) fn nan_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    check_unary(args)?;

    match cx.numeric_is_nan(args[0]) {
        Some(is_nan) => Ok(Value::boolean(is_nan)),
        None => Err(EvalError::TypeError(format!(
            "nan? expects a number, got {}",
            cx.type_name(args[0])
        ))),
    }
}
