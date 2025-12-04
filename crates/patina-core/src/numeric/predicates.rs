//! Numeric predicates for Value
//!
//! Implements R7RS numeric predicates: number?, real?, integer?, exact?, inexact?,
//! nan?, infinite?, finite?, zero?, positive?, negative?

use crate::value::Value;
use num_bigint::Sign;
use num_traits::{Signed, Zero};

impl Value {
    /// Returns true if this is any numeric type
    /// (integer, biginteger, rational, real, or complex)
    #[inline]
    pub fn is_number(&self) -> bool {
        matches!(
            self,
            Value::Integer(_)
                | Value::BigInteger(_)
                | Value::Rational(_)
                | Value::Real(_)
                | Value::Complex(_)
        )
    }

    /// Returns true if this is a real number.
    ///
    /// R7RS: A complex number with exact zero imaginary part IS real.
    /// A complex number with inexact zero imaginary part is NOT real.
    #[inline]
    pub fn is_real(&self) -> bool {
        match self {
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) | Value::Real(_) => true,
            Value::Complex(parts) => {
                let (_, ref imag) = **parts;
                // Only exact zero imaginary makes it real
                is_exact_zero(imag)
            }
            _ => false,
        }
    }

    /// Returns true if this represents an integer value.
    ///
    /// This includes:
    /// - Exact integers (Integer, BigInteger)
    /// - Rationals that simplify to integers (e.g., 6/2)
    /// - Inexact integers (e.g., 3.0)
    #[inline]
    pub fn is_integer_value(&self) -> bool {
        match self {
            Value::Integer(_) | Value::BigInteger(_) => true,
            Value::Rational(r) => r.is_integer(),
            Value::Real(f) => f.is_finite() && f.trunc() == *f,
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                is_exact_zero(imag) && real.is_integer_value()
            }
            _ => false,
        }
    }

    /// Returns true if this is an exact number.
    ///
    /// Exact numbers: Integer, BigInteger, Rational, and Complex with exact parts.
    #[inline]
    pub fn is_exact(&self) -> bool {
        match self {
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => true,
            Value::Real(_) => false,
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                real.is_exact() && imag.is_exact()
            }
            _ => false,
        }
    }

    /// Returns true if this is an inexact number.
    ///
    /// Inexact numbers: Real, and Complex with any inexact part.
    #[inline]
    pub fn is_inexact(&self) -> bool {
        match self {
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => false,
            Value::Real(_) => true,
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                real.is_inexact() || imag.is_inexact()
            }
            _ => false,
        }
    }

    /// Returns true if this is NaN.
    ///
    /// Only Real can be NaN. Complex is NaN if either part is NaN.
    #[inline]
    pub fn is_nan(&self) -> bool {
        match self {
            Value::Real(f) => f.is_nan(),
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                real.is_nan() || imag.is_nan()
            }
            _ => false,
        }
    }

    /// Returns true if this is infinite (+inf.0 or -inf.0).
    ///
    /// Only Real can be infinite. Complex is infinite if either part is infinite.
    #[inline]
    pub fn is_infinite(&self) -> bool {
        match self {
            Value::Real(f) => f.is_infinite(),
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                real.is_infinite() || imag.is_infinite()
            }
            _ => false,
        }
    }

    /// Returns true if this is a finite number (not NaN, not infinite).
    ///
    /// Exact numbers are always finite.
    #[inline]
    pub fn is_finite(&self) -> bool {
        match self {
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => true,
            Value::Real(f) => f.is_finite(),
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                real.is_finite() && imag.is_finite()
            }
            _ => false,
        }
    }

    /// Returns true if this is zero (exact or inexact).
    #[inline]
    pub fn is_numeric_zero(&self) -> bool {
        match self {
            Value::Integer(n) => *n == 0,
            Value::BigInteger(n) => n.sign() == Sign::NoSign,
            Value::Rational(r) => r.is_zero(),
            Value::Real(f) => *f == 0.0,
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                real.is_numeric_zero() && imag.is_numeric_zero()
            }
            _ => false,
        }
    }

    /// Returns true if this is negative.
    ///
    /// Only meaningful for real numbers. Returns false for complex.
    #[inline]
    pub fn is_negative(&self) -> bool {
        match self {
            Value::Integer(n) => *n < 0,
            Value::BigInteger(n) => n.sign() == Sign::Minus,
            Value::Rational(r) => r.is_negative(),
            Value::Real(f) => *f < 0.0,
            _ => false,
        }
    }

    /// Returns true if this is positive.
    ///
    /// Only meaningful for real numbers. Returns false for complex.
    #[inline]
    pub fn is_positive(&self) -> bool {
        match self {
            Value::Integer(n) => *n > 0,
            Value::BigInteger(n) => n.sign() == Sign::Plus,
            Value::Rational(r) => r.is_positive(),
            Value::Real(f) => *f > 0.0,
            _ => false,
        }
    }
}

/// Helper: Check if a value is exact zero (not inexact 0.0)
///
/// This is crucial for determining if a complex number is "real" per R7RS.
#[inline]
pub(crate) fn is_exact_zero(v: &Value) -> bool {
    match v {
        Value::Integer(n) => *n == 0,
        Value::BigInteger(n) => n.sign() == Sign::NoSign,
        Value::Rational(r) => r.is_zero(),
        // Inexact 0.0 is NOT exact zero
        Value::Real(_) => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;
    use num_rational::BigRational;

    #[test]
    fn test_is_number() {
        assert!(Value::Integer(42).is_number());
        assert!(Value::Real(2.75).is_number());
        assert!(!Value::Boolean(true).is_number());
        assert!(!Value::Null.is_number());
    }

    #[test]
    fn test_is_exact() {
        assert!(Value::Integer(42).is_exact());
        assert!(Value::BigInteger(BigInt::from(100)).is_exact());
        assert!(Value::Rational(BigRational::new(1.into(), 2.into())).is_exact());
        assert!(!Value::Real(2.75).is_exact());
    }

    #[test]
    fn test_is_inexact() {
        assert!(!Value::Integer(42).is_inexact());
        assert!(Value::Real(2.75).is_inexact());
    }

    #[test]
    fn test_is_nan() {
        assert!(!Value::Integer(42).is_nan());
        assert!(!Value::Real(2.75).is_nan());
        assert!(Value::Real(f64::NAN).is_nan());
    }

    #[test]
    fn test_is_infinite() {
        assert!(!Value::Integer(42).is_infinite());
        assert!(!Value::Real(2.75).is_infinite());
        assert!(Value::Real(f64::INFINITY).is_infinite());
        assert!(Value::Real(f64::NEG_INFINITY).is_infinite());
    }

    #[test]
    fn test_is_numeric_zero() {
        assert!(Value::Integer(0).is_numeric_zero());
        assert!(!Value::Integer(1).is_numeric_zero());
        assert!(Value::Real(0.0).is_numeric_zero());
        assert!(!Value::Real(0.1).is_numeric_zero());
    }

    #[test]
    fn test_is_real_with_complex() {
        // Complex with exact zero imaginary IS real
        let exact_zero_imag = Value::Complex(Box::new((Value::Integer(5), Value::Integer(0))));
        assert!(exact_zero_imag.is_real());

        // Complex with inexact zero imaginary is NOT real
        let inexact_zero_imag = Value::Complex(Box::new((Value::Real(5.0), Value::Real(0.0))));
        assert!(!inexact_zero_imag.is_real());

        // Complex with non-zero imaginary is NOT real
        let non_zero_imag = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        assert!(!non_zero_imag.is_real());
    }

    #[test]
    fn test_complex_exactness() {
        // Complex with exact parts is exact
        let exact_complex = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        assert!(exact_complex.is_exact());
        assert!(!exact_complex.is_inexact());

        // Complex with inexact parts is inexact
        let inexact_complex = Value::Complex(Box::new((Value::Real(3.0), Value::Real(4.0))));
        assert!(!inexact_complex.is_exact());
        assert!(inexact_complex.is_inexact());

        // Complex with mixed parts is inexact
        let mixed_complex = Value::Complex(Box::new((Value::Integer(3), Value::Real(4.0))));
        assert!(!mixed_complex.is_exact());
        assert!(mixed_complex.is_inexact());
    }
}
