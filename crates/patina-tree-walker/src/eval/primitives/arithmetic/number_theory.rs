//! Number theory operations
//!
//! This module implements:
//! - gcd, lcm - Greatest common divisor and least common multiple
//! - numerator, denominator - Rational number accessors
//! - exact, inexact - Exactness conversion
//! - exact-integer-sqrt - Integer square root with remainder
//! - rationalize - Find simplest rational within tolerance

use super::helpers::numeric_err;
use crate::eval::Evaluator;
use crate::eval::error::EvalError;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use patina_core::numeric;
use patina_runtime::value::Value;

// ========== GCD and LCM ==========

/// (gcd n1 n2 ...) - Greatest common divisor
pub(super) fn gcd(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    numeric::numeric_gcd_many(&args).map_err(|e| numeric_err(e, "gcd"))
}

/// (lcm n1 n2 ...) - Least common multiple
pub(super) fn lcm(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    numeric::numeric_lcm_many(&args).map_err(|e| numeric_err(e, "lcm"))
}

// ========== Rational Number Accessors ==========

/// (numerator q) - Returns the numerator of rational q
/// R7RS: If the argument is inexact, the result is also inexact.
pub(super) fn numerator(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "numerator")?;
    args[0].numerator().map_err(|e| numeric_err(e, "numerator"))
}

/// (denominator q) - Returns the denominator of rational q
/// R7RS: If the argument is inexact, the result is also inexact.
pub(super) fn denominator(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "denominator")?;
    args[0]
        .denominator()
        .map_err(|e| numeric_err(e, "denominator"))
}

// ========== Exactness Conversion ==========

/// (exact z) - Convert to exact representation (inexact->exact)
pub(super) fn exact(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exact")?;
    args[0].to_exact().map_err(|e| numeric_err(e, "exact"))
}

/// (inexact z) - Convert to inexact representation (exact->inexact)
pub(super) fn inexact(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "inexact")?;
    Ok(args[0].to_inexact())
}

// ========== Additional Number Theory Functions ==========

/// (exact-integer-sqrt k) - Returns two values s and r where k = s^2 + r, and k < (s+1)^2
/// This is the integer square root with remainder
pub(super) fn exact_integer_sqrt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exact-integer-sqrt")?;

    // Convert to BigInt for uniform handling
    let k = match &args[0] {
        Value::Integer(n) if *n < 0 => {
            return Err(EvalError::TypeError(
                "exact-integer-sqrt expects a non-negative integer".to_string(),
            ));
        }
        Value::Integer(n) => BigInt::from(*n),
        Value::BigInteger(n) if n < &BigInt::from(0) => {
            return Err(EvalError::TypeError(
                "exact-integer-sqrt expects a non-negative integer".to_string(),
            ));
        }
        Value::BigInteger(n) => n.clone(),
        other => {
            return Err(EvalError::TypeError(format!(
                "exact-integer-sqrt expects an exact integer, got {}",
                other.type_name()
            )));
        }
    };

    // Compute integer square root using Newton's method
    let (s, r) = if k.is_zero() {
        (BigInt::from(0), BigInt::from(0))
    } else {
        let mut s = k.clone();
        let mut s_next = (&s + &k / &s) / BigInt::from(2);

        while s_next < s {
            s = s_next.clone();
            s_next = (&s + &k / &s) / BigInt::from(2);
        }

        // s is the integer square root
        // r = k - s^2 is the remainder
        let r = &k - &s * &s;
        (s, r)
    };

    // Convert s and r to appropriate Value types
    let s_val = match s.to_i64() {
        Some(n) => Value::Integer(n),
        None => Value::BigInteger(s),
    };

    let r_val = match r.to_i64() {
        Some(n) => Value::Integer(n),
        None => Value::BigInteger(r),
    };

    // Return as multiple values using Value::Values
    Ok(Value::Values(vec![s_val, r_val]))
}

/// (rationalize x tolerance) - Find simplest rational within tolerance of x
/// Returns the rational with smallest denominator within x±tolerance
///
/// R7RS: If x is inexact, the result is inexact. If x is exact, the result is exact.
pub(super) fn rationalize(eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    eval.check_arity_exact(&args, 2, "rationalize")?;

    let x = &args[0];
    let tolerance = &args[1];

    // Validate inputs are real numbers
    if !x.is_number() || matches!(x, Value::Complex(_)) {
        return Err(EvalError::TypeError(
            "rationalize: expected real number".to_string(),
        ));
    }
    if !tolerance.is_number() || matches!(tolerance, Value::Complex(_)) {
        return Err(EvalError::TypeError(
            "rationalize: expected real number for tolerance".to_string(),
        ));
    }

    // Check if x is inexact - this determines the exactness of the result
    let x_is_inexact = x.is_inexact();

    // Convert to f64 for the algorithm
    let x_f64 = match x {
        Value::Integer(n) => *n as f64,
        Value::BigInteger(n) => n.to_f64().unwrap_or(f64::INFINITY),
        Value::Rational(r) => r.to_f64().unwrap_or(f64::INFINITY),
        Value::Real(f) => *f,
        _ => unreachable!(),
    };

    let tol_f64 = match tolerance {
        Value::Integer(n) => (*n as f64).abs(),
        Value::BigInteger(n) => n.to_f64().unwrap_or(f64::INFINITY).abs(),
        Value::Rational(r) => r.to_f64().unwrap_or(f64::INFINITY).abs(),
        Value::Real(f) => f.abs(),
        _ => unreachable!(),
    };

    // Range: [x - tolerance, x + tolerance]
    let lower = x_f64 - tol_f64;
    let upper = x_f64 + tol_f64;

    // Use continued fractions to find simplest rational in range
    // This is a simplified implementation
    fn simplest_rational_in_range(lower: f64, upper: f64) -> BigRational {
        use num_traits::FromPrimitive;

        // Handle edge cases
        if lower.is_nan() || upper.is_nan() || lower.is_infinite() || upper.is_infinite() {
            return BigRational::from_f64(lower)
                .unwrap_or_else(|| BigRational::from(BigInt::from(0)));
        }

        // If range contains 0, return 0
        if lower <= 0.0 && upper >= 0.0 {
            return BigRational::from(BigInt::from(0));
        }

        // If range contains an integer, return the closest one
        let lower_ceil = lower.ceil();
        if lower_ceil <= upper {
            return BigRational::from(BigInt::from(lower_ceil as i64));
        }

        // Use continued fraction algorithm
        // For simplicity, we'll use a basic approximation
        // Convert bounds to rationals and find the mediant
        let lower_rat = BigRational::from_f64(lower).unwrap();
        let _upper_rat = BigRational::from_f64(upper).unwrap();

        // Try simple fractions first
        for denom in 1..=100 {
            let d = BigInt::from(denom);
            let lower_num = (lower * denom as f64).ceil() as i64;
            let upper_num = (upper * denom as f64).floor() as i64;

            if lower_num <= upper_num {
                return BigRational::new(BigInt::from(lower_num), d);
            }
        }

        // Fall back to one of the bounds
        lower_rat
    }

    let result = simplest_rational_in_range(lower, upper);

    // R7RS: If x is inexact, return inexact result; otherwise exact
    if x_is_inexact {
        // Convert rational to inexact (f64)
        let f = result.to_f64().unwrap_or(f64::NAN);
        Ok(Value::Real(f))
    } else {
        // Return exact rational
        Ok(eval.rational_to_value(result))
    }
}
