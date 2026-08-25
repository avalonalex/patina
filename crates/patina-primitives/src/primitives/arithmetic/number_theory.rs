//! Number theory operations
//!
//! This module implements:
//! - gcd, lcm - Greatest common divisor and least common multiple
//! - numerator, denominator - Rational number accessors
//! - exact, inexact - Exactness conversion
//! - exact-integer-sqrt - Integer square root with remainder
//! - rationalize - Find simplest rational within tolerance
//!
//! All operations use Heap methods which have built-in fixnum fast paths.

use super::helpers::{numeric_err, rational_to_tagged};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

// ========== GCD and LCM ==========

/// (gcd n1 n2 ...) - Greatest common divisor
/// Uses Heap::gcd_many with built-in fixnum fast path.
pub(super) fn gcd(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    heap.borrow_mut()
        .gcd_many(args)
        .map_err(|e| numeric_err(e, "gcd"))
}

/// (lcm n1 n2 ...) - Least common multiple
/// Uses Heap::lcm_many with built-in fixnum fast path.
pub(super) fn lcm(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    heap.borrow_mut()
        .lcm_many(args)
        .map_err(|e| numeric_err(e, "lcm"))
}

// ========== Rational Number Accessors ==========

/// (numerator q) - Returns the numerator of rational q
/// Uses Heap::numerator with built-in fixnum fast path.
pub(super) fn numerator(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .numerator(args[0])
        .map_err(|e| numeric_err(e, "numerator"))
}

/// (denominator q) - Returns the denominator of rational q
/// Uses Heap::denominator with built-in fixnum fast path.
pub(super) fn denominator(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .denominator(args[0])
        .map_err(|e| numeric_err(e, "denominator"))
}

// ========== Exactness Conversion ==========

/// (exact z) - Convert to exact representation (inexact->exact)
/// Uses Heap::to_exact with built-in fixnum fast path.
pub(super) fn exact(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    heap.borrow_mut()
        .to_exact(args[0])
        .map_err(|e| numeric_err(e, "exact"))
}

/// (inexact z) - Convert to inexact representation (exact->inexact)
/// Uses Heap::to_inexact with built-in fixnum fast path.
pub(super) fn inexact(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    Ok(heap.borrow_mut().to_inexact(args[0]))
}

// ========== Additional Number Theory Functions ==========

/// (exact-integer-sqrt k) - Returns two values s and r where k = s^2 + r, and k < (s+1)^2
/// This is the integer square root with remainder
pub(super) fn exact_integer_sqrt(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Fast path for fixnums
    if args[0].is_fixnum() {
        let n = args[0].as_fixnum_unchecked();
        if n < 0 {
            return Err(EvalError::TypeError(
                "exact-integer-sqrt expects a non-negative integer".to_string(),
            ));
        }
        let s = (n as f64).sqrt().floor() as i64;
        let r = n - s * s;
        let s_tv = TaggedValue::fixnum(s);
        let r_tv = TaggedValue::fixnum(r);
        return Ok(heap.borrow_mut().alloc_values(vec![s_tv, r_tv]));
    }

    // Slow path: BigInt
    let k = {
        let heap_ref = heap.borrow();
        if let Some(n) = heap_ref.get_bigint(args[0]) {
            if n < &BigInt::from(0) {
                return Err(EvalError::TypeError(
                    "exact-integer-sqrt expects a non-negative integer".to_string(),
                ));
            }
            n.clone()
        } else {
            return Err(EvalError::TypeError(format!(
                "exact-integer-sqrt expects an exact integer, got {}",
                heap_ref.type_name(args[0])
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

        let r = &k - &s * &s;
        (s, r)
    };

    // Convert s and r to TaggedValues directly
    let mut heap_mut = heap.borrow_mut();
    let s_tv = match s.to_i64().filter(|n| TaggedValue::fits_fixnum(*n)) {
        Some(n) => TaggedValue::fixnum(n),
        None => heap_mut.alloc_bigint(s),
    };
    let r_tv = match r.to_i64().filter(|n| TaggedValue::fits_fixnum(*n)) {
        Some(n) => TaggedValue::fixnum(n),
        None => heap_mut.alloc_bigint(r),
    };
    Ok(heap_mut.alloc_values(vec![s_tv, r_tv]))
}

/// (rationalize x tolerance) - Find simplest rational within tolerance of x
/// Returns the rational with smallest denominator within x±tolerance
///
/// R7RS: If x is inexact, the result is inexact. If x is exact, the result is exact.
pub(super) fn rationalize(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    // Validate inputs are real numbers (number but not complex)
    {
        let heap_ref = heap.borrow();
        if !heap_ref.is_number(args[0]) || heap_ref.is_complex(args[0]) {
            return Err(EvalError::TypeError(
                "rationalize: expected real number".to_string(),
            ));
        }
        if !heap_ref.is_number(args[1]) || heap_ref.is_complex(args[1]) {
            return Err(EvalError::TypeError(
                "rationalize: expected real number for tolerance".to_string(),
            ));
        }
    }

    // R7RS 6.2.6: the result is inexact if either argument is, so
    // (rationalize 3 +inf.0) is 0.0, not 0.
    let x_is_inexact = {
        let h = heap.borrow();
        h.is_inexact_number(args[0]) || h.is_inexact_number(args[1])
    };

    // Helper to extract f64 from a real-valued TaggedValue
    let tv_to_f64 = |tv: TaggedValue| -> f64 {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() as f64;
        }
        let heap_ref = heap.borrow();
        if let Some(f) = heap_ref.get_real(tv) {
            return f;
        }
        if let Some(n) = heap_ref.get_bigint(tv) {
            return n.to_f64().unwrap_or(f64::INFINITY);
        }
        if let Some(r) = heap_ref.get_rational(tv) {
            return r.to_f64().unwrap_or(f64::INFINITY);
        }
        unreachable!()
    };

    let x_f64 = tv_to_f64(args[0]);
    let tol_f64 = tv_to_f64(args[1]).abs();

    // The infinities, per R7RS 6.2.6's examples: (rationalize +inf.0 3) is
    // +inf.0 — the simplest number within 3 of infinity is infinity;
    // (rationalize 3 +inf.0) is 0.0 — every number is within an infinite
    // tolerance and 0 is the simplest; (rationalize +inf.0 +inf.0) is +nan.0.
    // The interval arithmetic below turns all of these into 0.
    if x_f64.is_nan() || tol_f64.is_nan() || (x_f64.is_infinite() && tol_f64.is_infinite()) {
        return Ok(heap.borrow_mut().alloc_real(f64::NAN));
    }
    if x_f64.is_infinite() {
        return Ok(heap.borrow_mut().alloc_real(x_f64));
    }
    if tol_f64.is_infinite() {
        return Ok(heap.borrow_mut().alloc_real(0.0));
    }

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

    let result_rational = simplest_rational_in_range(lower, upper);

    // R7RS: If x is inexact, return inexact result; otherwise exact
    if x_is_inexact {
        let f = result_rational.to_f64().unwrap_or(f64::NAN);
        Ok(heap.borrow_mut().alloc_real(f))
    } else {
        Ok(rational_to_tagged(heap, result_rational))
    }
}
