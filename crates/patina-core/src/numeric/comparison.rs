//! Numeric comparison operations: =, <, >, <=, >=, max, min
//!
//! R7RS Section 6.2.6

use super::error::NumericError;
use super::exactness::{apply_contagion, result_exactness};
use crate::value::Value;
use num_traits::ToPrimitive;

impl Value {
    /// Numeric equality (=)
    ///
    /// NaN is not equal to anything, including itself.
    /// Works for complex numbers.
    pub fn numeric_eq(&self, other: &Value) -> Result<bool, NumericError> {
        // NaN is never equal to anything
        if self.is_nan() || other.is_nan() {
            return Ok(false);
        }

        numeric_eq_impl(self, other)
    }

    /// Less than (<)
    ///
    /// Returns error for complex numbers.
    /// Returns false if either is NaN.
    pub fn numeric_lt(&self, other: &Value) -> Result<bool, NumericError> {
        // NaN comparisons always return false
        if self.is_nan() || other.is_nan() {
            return Ok(false);
        }

        // Complex numbers can't be ordered
        if matches!(self, Value::Complex(_)) || matches!(other, Value::Complex(_)) {
            return Err(NumericError::NotReal {
                got: "complex".to_string(),
            });
        }

        compare_real(self, other, |a, b| a < b, |a, b| a < b)
    }

    /// Greater than (>)
    pub fn numeric_gt(&self, other: &Value) -> Result<bool, NumericError> {
        // NaN comparisons always return false
        if self.is_nan() || other.is_nan() {
            return Ok(false);
        }

        // Complex numbers can't be ordered
        if matches!(self, Value::Complex(_)) || matches!(other, Value::Complex(_)) {
            return Err(NumericError::NotReal {
                got: "complex".to_string(),
            });
        }

        compare_real(self, other, |a, b| a > b, |a, b| a > b)
    }

    /// Less than or equal (<=)
    pub fn numeric_le(&self, other: &Value) -> Result<bool, NumericError> {
        // NaN comparisons always return false
        if self.is_nan() || other.is_nan() {
            return Ok(false);
        }

        // Complex numbers can't be ordered
        if matches!(self, Value::Complex(_)) || matches!(other, Value::Complex(_)) {
            return Err(NumericError::NotReal {
                got: "complex".to_string(),
            });
        }

        compare_real(self, other, |a, b| a <= b, |a, b| a <= b)
    }

    /// Greater than or equal (>=)
    pub fn numeric_ge(&self, other: &Value) -> Result<bool, NumericError> {
        // NaN comparisons always return false
        if self.is_nan() || other.is_nan() {
            return Ok(false);
        }

        // Complex numbers can't be ordered
        if matches!(self, Value::Complex(_)) || matches!(other, Value::Complex(_)) {
            return Err(NumericError::NotReal {
                got: "complex".to_string(),
            });
        }

        compare_real(self, other, |a, b| a >= b, |a, b| a >= b)
    }

    /// Find maximum of two numbers, with NaN propagation.
    ///
    /// If either operand is NaN, returns NaN.
    /// Applies inexact contagion.
    pub fn numeric_max(&self, other: &Value) -> Result<Value, NumericError> {
        // NaN propagation (like Chez Scheme)
        if self.is_nan() || other.is_nan() {
            return Ok(Value::Real(f64::NAN));
        }

        // Complex numbers can't be compared for max
        if matches!(self, Value::Complex(_)) || matches!(other, Value::Complex(_)) {
            return Err(NumericError::NotReal {
                got: "complex".to_string(),
            });
        }

        let exactness = result_exactness(self, other);

        if self.numeric_gt(other)? {
            Ok(apply_contagion(self.clone(), exactness))
        } else {
            Ok(apply_contagion(other.clone(), exactness))
        }
    }

    /// Find minimum of two numbers, with NaN propagation.
    ///
    /// If either operand is NaN, returns NaN.
    /// Applies inexact contagion.
    pub fn numeric_min(&self, other: &Value) -> Result<Value, NumericError> {
        // NaN propagation (like Chez Scheme)
        if self.is_nan() || other.is_nan() {
            return Ok(Value::Real(f64::NAN));
        }

        // Complex numbers can't be compared for min
        if matches!(self, Value::Complex(_)) || matches!(other, Value::Complex(_)) {
            return Err(NumericError::NotReal {
                got: "complex".to_string(),
            });
        }

        let exactness = result_exactness(self, other);

        if self.numeric_lt(other)? {
            Ok(apply_contagion(self.clone(), exactness))
        } else {
            Ok(apply_contagion(other.clone(), exactness))
        }
    }
}

// =============================================================================
// Implementation helpers
// =============================================================================

/// Numeric equality implementation
fn numeric_eq_impl(a: &Value, b: &Value) -> Result<bool, NumericError> {
    use num_bigint::BigInt;
    use num_rational::BigRational;
    use Value::*;

    match (a, b) {
        // Same type comparisons
        (Integer(x), Integer(y)) => Ok(x == y),
        (BigInteger(x), BigInteger(y)) => Ok(x == y),
        (Rational(x), Rational(y)) => Ok(x == y),
        (Real(x), Real(y)) => Ok(x == y),

        // Integer vs BigInteger
        (Integer(x), BigInteger(y)) | (BigInteger(y), Integer(x)) => Ok(&BigInt::from(*x) == y),

        // Integer vs Rational
        (Integer(x), Rational(y)) | (Rational(y), Integer(x)) => {
            Ok(&BigRational::from(BigInt::from(*x)) == y)
        }

        // BigInteger vs Rational
        (BigInteger(x), Rational(y)) | (Rational(y), BigInteger(x)) => {
            Ok(&BigRational::from(x.clone()) == y)
        }

        // Real vs exact types - carefully handle precision issues
        // If the real is a whole number that can be exactly represented,
        // convert to exact and compare exactly
        (Real(x), Integer(y)) | (Integer(y), Real(x)) => compare_real_exact(*x, &BigInt::from(*y)),
        (Real(x), BigInteger(y)) | (BigInteger(y), Real(x)) => compare_real_exact(*x, y),
        (Real(x), Rational(y)) | (Rational(y), Real(x)) => {
            // For rationals, convert to f64 and compare
            // But check if the real is exactly representable as this rational
            if let Some(exact_real) = BigRational::from_float(*x) {
                Ok(&exact_real == y)
            } else {
                Ok(*x == y.to_f64().unwrap_or(f64::INFINITY))
            }
        }

        // Complex equality
        (Complex(x), Complex(y)) => {
            let (ref xr, ref xi) = **x;
            let (ref yr, ref yi) = **y;
            Ok(numeric_eq_impl(xr, yr)? && numeric_eq_impl(xi, yi)?)
        }

        // Complex vs real (real is complex with zero imaginary)
        (Complex(x), other) | (other, Complex(x)) if other.is_number() => {
            let (ref xr, ref xi) = **x;
            // Complex equals real iff imaginary is zero and real parts equal
            Ok(xi.is_numeric_zero() && numeric_eq_impl(xr, other)?)
        }

        // Error cases
        (x, _) if !x.is_number() => Err(NumericError::NotANumber {
            got: x.type_name().to_string(),
        }),
        (_, y) if !y.is_number() => Err(NumericError::NotANumber {
            got: y.type_name().to_string(),
        }),
        _ => unreachable!("All numeric cases should be covered"),
    }
}

/// Compare a Real (f64) to an exact integer (BigInt)
/// Handles the case where the exact integer cannot be exactly represented in f64
fn compare_real_exact(real: f64, exact: &num_bigint::BigInt) -> Result<bool, NumericError> {
    use num_traits::{FromPrimitive, ToPrimitive};

    // If real is not a whole number, they can't be equal
    if real.fract() != 0.0 {
        return Ok(false);
    }

    // If real is infinite or NaN, they can't be equal
    if !real.is_finite() {
        return Ok(false);
    }

    // Try to convert the real to a BigInt
    if let Some(real_as_bigint) = num_bigint::BigInt::from_f64(real) {
        // Compare exactly
        Ok(&real_as_bigint == exact)
    } else {
        // Can't convert - fall back to f64 comparison
        Ok(real == exact.to_f64().unwrap_or(f64::INFINITY))
    }
}

/// Compare two real numbers using provided comparison functions
fn compare_real<F, G>(a: &Value, b: &Value, cmp_f64: F, cmp_rat: G) -> Result<bool, NumericError>
where
    F: Fn(f64, f64) -> bool,
    G: Fn(&num_rational::BigRational, &num_rational::BigRational) -> bool,
{
    use num_bigint::BigInt;
    use num_rational::BigRational;
    use Value::*;

    // If either is a Real, use f64 comparison
    if matches!(a, Real(_)) || matches!(b, Real(_)) {
        let af = a.to_f64()?;
        let bf = b.to_f64()?;
        return Ok(cmp_f64(af, bf));
    }

    // Both are exact - use rational comparison for precision
    match (a, b) {
        (Integer(x), Integer(y)) => Ok(cmp_rat(
            &BigRational::from(BigInt::from(*x)),
            &BigRational::from(BigInt::from(*y)),
        )),
        (Integer(x), BigInteger(y)) => Ok(cmp_rat(
            &BigRational::from(BigInt::from(*x)),
            &BigRational::from(y.clone()),
        )),
        (BigInteger(x), Integer(y)) => Ok(cmp_rat(
            &BigRational::from(x.clone()),
            &BigRational::from(BigInt::from(*y)),
        )),
        (BigInteger(x), BigInteger(y)) => Ok(cmp_rat(
            &BigRational::from(x.clone()),
            &BigRational::from(y.clone()),
        )),
        (Integer(x), Rational(y)) => Ok(cmp_rat(&BigRational::from(BigInt::from(*x)), y)),
        (Rational(x), Integer(y)) => Ok(cmp_rat(x, &BigRational::from(BigInt::from(*y)))),
        (BigInteger(x), Rational(y)) => Ok(cmp_rat(&BigRational::from(x.clone()), y)),
        (Rational(x), BigInteger(y)) => Ok(cmp_rat(x, &BigRational::from(y.clone()))),
        (Rational(x), Rational(y)) => Ok(cmp_rat(x, y)),

        // Shouldn't reach here since we handle Real above
        _ => {
            let af = a.to_f64()?;
            let bf = b.to_f64()?;
            Ok(cmp_f64(af, bf))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_integer(v: &Value, expected: i64) {
        match v {
            Value::Integer(n) => assert_eq!(*n, expected),
            _ => panic!("Expected Integer, got {:?}", v),
        }
    }

    #[test]
    fn test_numeric_eq_integers() {
        assert!(Value::Integer(5).numeric_eq(&Value::Integer(5)).unwrap());
        assert!(!Value::Integer(5).numeric_eq(&Value::Integer(6)).unwrap());
    }

    #[test]
    fn test_numeric_eq_mixed() {
        assert!(Value::Integer(5).numeric_eq(&Value::Real(5.0)).unwrap());
        assert!(!Value::Integer(5).numeric_eq(&Value::Real(5.1)).unwrap());
    }

    #[test]
    fn test_numeric_eq_nan() {
        let nan = Value::Real(f64::NAN);
        assert!(!nan.numeric_eq(&nan).unwrap());
        assert!(!nan.numeric_eq(&Value::Integer(5)).unwrap());
    }

    #[test]
    fn test_numeric_lt() {
        assert!(Value::Integer(3).numeric_lt(&Value::Integer(5)).unwrap());
        assert!(!Value::Integer(5).numeric_lt(&Value::Integer(3)).unwrap());
        assert!(!Value::Integer(5).numeric_lt(&Value::Integer(5)).unwrap());
    }

    #[test]
    fn test_numeric_lt_nan() {
        let nan = Value::Real(f64::NAN);
        assert!(!nan.numeric_lt(&Value::Integer(5)).unwrap());
        assert!(!Value::Integer(5).numeric_lt(&nan).unwrap());
    }

    #[test]
    fn test_numeric_max() {
        let a = Value::Integer(3);
        let b = Value::Integer(5);
        assert_integer(&a.numeric_max(&b).unwrap(), 5);
        assert_integer(&b.numeric_max(&a).unwrap(), 5);
    }

    #[test]
    fn test_numeric_max_inexact_contagion() {
        let a = Value::Integer(3);
        let b = Value::Real(5.0);
        // Result should be inexact even though max value came from exact
        let result = a.numeric_max(&b).unwrap();
        assert!(result.is_inexact());
    }

    #[test]
    fn test_numeric_max_nan_propagation() {
        let a = Value::Integer(5);
        let nan = Value::Real(f64::NAN);
        assert!(a.numeric_max(&nan).unwrap().is_nan());
        assert!(nan.numeric_max(&a).unwrap().is_nan());
    }

    #[test]
    fn test_numeric_min() {
        let a = Value::Integer(3);
        let b = Value::Integer(5);
        assert_integer(&a.numeric_min(&b).unwrap(), 3);
        assert_integer(&b.numeric_min(&a).unwrap(), 3);
    }

    #[test]
    fn test_numeric_min_nan_propagation() {
        let a = Value::Integer(5);
        let nan = Value::Real(f64::NAN);
        assert!(a.numeric_min(&nan).unwrap().is_nan());
        assert!(nan.numeric_min(&a).unwrap().is_nan());
    }

    #[test]
    fn test_complex_equality() {
        let a = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        let b = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        let c = Value::Complex(Box::new((Value::Integer(3), Value::Integer(5))));
        assert!(a.numeric_eq(&b).unwrap());
        assert!(!a.numeric_eq(&c).unwrap());
    }

    #[test]
    fn test_complex_comparison_error() {
        let a = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        let b = Value::Complex(Box::new((Value::Integer(5), Value::Integer(6))));
        assert!(a.numeric_lt(&b).is_err());
    }
}
