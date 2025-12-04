//! Core arithmetic operations: add, subtract, multiply, divide, negate, abs
//!
//! Implements automatic type promotion and exactness contagion.

use super::error::NumericError;
use super::exactness::{apply_contagion, result_exactness};
use crate::value::Value;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive};

impl Value {
    /// Add two numbers with automatic type promotion.
    ///
    /// Returns NaN if either operand is NaN (NaN propagation).
    /// Applies inexact contagion.
    pub fn numeric_add(&self, other: &Value) -> Result<Value, NumericError> {
        // NaN propagation
        if self.is_nan() || other.is_nan() {
            return Ok(Value::Real(f64::NAN));
        }

        let exactness = result_exactness(self, other);
        let result = add_impl(self, other)?;
        Ok(apply_contagion(result, exactness))
    }

    /// Subtract: self - other
    pub fn numeric_sub(&self, other: &Value) -> Result<Value, NumericError> {
        // NaN propagation
        if self.is_nan() || other.is_nan() {
            return Ok(Value::Real(f64::NAN));
        }

        let exactness = result_exactness(self, other);
        let result = sub_impl(self, other)?;
        Ok(apply_contagion(result, exactness))
    }

    /// Multiply two numbers
    pub fn numeric_mul(&self, other: &Value) -> Result<Value, NumericError> {
        // NaN propagation
        if self.is_nan() || other.is_nan() {
            return Ok(Value::Real(f64::NAN));
        }

        let exactness = result_exactness(self, other);
        let result = mul_impl(self, other)?;
        Ok(apply_contagion(result, exactness))
    }

    /// Divide: self / other
    ///
    /// Returns error for division by exact zero.
    /// Returns infinity for division by inexact zero.
    pub fn numeric_div(&self, other: &Value) -> Result<Value, NumericError> {
        // NaN propagation
        if self.is_nan() || other.is_nan() {
            return Ok(Value::Real(f64::NAN));
        }

        // Check for division by zero
        // Only error if BOTH operands are exact. If either is inexact, return infinity or NaN.
        if other.is_numeric_zero() {
            if self.is_exact() && other.is_exact() {
                return Err(NumericError::DivisionByZero);
            }
            // If dividend is also zero (0.0/0.0), return NaN
            if self.is_numeric_zero() {
                return Ok(Value::Real(f64::NAN));
            }
            // Otherwise return infinity with appropriate sign
            let sign = if self.is_negative() { -1.0 } else { 1.0 };
            return Ok(Value::Real(sign * f64::INFINITY));
        }

        let exactness = result_exactness(self, other);
        let result = div_impl(self, other)?;
        Ok(apply_contagion(result, exactness))
    }

    /// Negate: -self
    pub fn numeric_neg(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(n) => match n.checked_neg() {
                Some(neg) => Ok(Value::Integer(neg)),
                None => Ok(Value::BigInteger(-BigInt::from(*n))),
            },
            Value::BigInteger(n) => Ok(Value::BigInteger(-n.clone())),
            Value::Rational(r) => Ok(Value::Rational(-r.clone())),
            Value::Real(f) => Ok(Value::Real(-f)),
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                Ok(Value::Complex(Box::new((
                    real.numeric_neg()?,
                    imag.numeric_neg()?,
                ))))
            }
            other => Err(NumericError::NotANumber {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Absolute value
    pub fn numeric_abs(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(n) => match n.checked_abs() {
                Some(abs) => Ok(Value::Integer(abs)),
                None => Ok(Value::BigInteger(BigInt::from(*n).abs())),
            },
            Value::BigInteger(n) => Ok(Value::BigInteger(n.abs())),
            Value::Rational(r) => Ok(Value::Rational(r.abs())),
            Value::Real(f) => Ok(Value::Real(f.abs())),
            Value::Complex(_) => {
                // abs of complex is magnitude
                self.magnitude()
            }
            other => Err(NumericError::NotANumber {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Convert to f64 (for operations that need it)
    pub(crate) fn to_f64(&self) -> Result<f64, NumericError> {
        match self {
            Value::Integer(n) => Ok(*n as f64),
            Value::BigInteger(n) => Ok(n.to_f64().unwrap_or(f64::INFINITY)),
            Value::Rational(r) => Ok(r.to_f64().unwrap_or(f64::INFINITY)),
            Value::Real(f) => Ok(*f),
            other => Err(NumericError::NotReal {
                got: other.type_name().to_string(),
            }),
        }
    }
}

// =============================================================================
// Implementation helpers
// =============================================================================

/// Add implementation without exactness contagion
fn add_impl(a: &Value, b: &Value) -> Result<Value, NumericError> {
    use Value::*;

    match (a, b) {
        // Integer + Integer (with overflow check)
        (Integer(x), Integer(y)) => match x.checked_add(*y) {
            Some(sum) => Ok(Integer(sum)),
            None => Ok(BigInteger(BigInt::from(*x) + BigInt::from(*y))),
        },

        // BigInteger combinations
        (BigInteger(x), Integer(y)) | (Integer(y), BigInteger(x)) => {
            Ok(BigInteger(x.clone() + BigInt::from(*y)))
        }
        (BigInteger(x), BigInteger(y)) => Ok(BigInteger(x.clone() + y.clone())),

        // Rational combinations
        (Rational(x), Rational(y)) => Ok(rational_to_value(x.clone() + y.clone())),
        (Rational(x), Integer(y)) | (Integer(y), Rational(x)) => Ok(rational_to_value(
            x.clone() + BigRational::from(BigInt::from(*y)),
        )),
        (Rational(x), BigInteger(y)) | (BigInteger(y), Rational(x)) => {
            Ok(rational_to_value(x.clone() + BigRational::from(y.clone())))
        }

        // Real combinations (inexact)
        (Real(x), Real(y)) => Ok(Real(x + y)),
        (Real(x), Integer(y)) | (Integer(y), Real(x)) => Ok(Real(x + *y as f64)),
        (Real(x), BigInteger(y)) | (BigInteger(y), Real(x)) => {
            Ok(Real(x + y.to_f64().unwrap_or(f64::INFINITY)))
        }
        (Real(x), Rational(y)) | (Rational(y), Real(x)) => {
            Ok(Real(x + y.to_f64().unwrap_or(f64::INFINITY)))
        }

        // Complex combinations
        (Complex(x), Complex(y)) => {
            let (ref xr, ref xi) = **x;
            let (ref yr, ref yi) = **y;
            let real = add_impl(xr, yr)?;
            let imag = add_impl(xi, yi)?;
            Ok(simplify_complex(real, imag))
        }
        (Complex(x), other) | (other, Complex(x)) if other.is_number() => {
            let (ref xr, ref xi) = **x;
            let real = add_impl(xr, other)?;
            Ok(simplify_complex(real, xi.clone()))
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

/// Subtract implementation without exactness contagion
fn sub_impl(a: &Value, b: &Value) -> Result<Value, NumericError> {
    use Value::*;

    match (a, b) {
        // Integer - Integer (with overflow check)
        (Integer(x), Integer(y)) => match x.checked_sub(*y) {
            Some(diff) => Ok(Integer(diff)),
            None => Ok(BigInteger(BigInt::from(*x) - BigInt::from(*y))),
        },

        // BigInteger combinations
        (BigInteger(x), Integer(y)) => Ok(BigInteger(x.clone() - BigInt::from(*y))),
        (Integer(x), BigInteger(y)) => Ok(BigInteger(BigInt::from(*x) - y.clone())),
        (BigInteger(x), BigInteger(y)) => Ok(BigInteger(x.clone() - y.clone())),

        // Rational combinations
        (Rational(x), Rational(y)) => Ok(rational_to_value(x.clone() - y.clone())),
        (Rational(x), Integer(y)) => Ok(rational_to_value(
            x.clone() - BigRational::from(BigInt::from(*y)),
        )),
        (Integer(x), Rational(y)) => Ok(rational_to_value(
            BigRational::from(BigInt::from(*x)) - y.clone(),
        )),
        (Rational(x), BigInteger(y)) => {
            Ok(rational_to_value(x.clone() - BigRational::from(y.clone())))
        }
        (BigInteger(x), Rational(y)) => {
            Ok(rational_to_value(BigRational::from(x.clone()) - y.clone()))
        }

        // Real combinations (inexact)
        (Real(x), Real(y)) => Ok(Real(x - y)),
        (Real(x), Integer(y)) => Ok(Real(x - *y as f64)),
        (Integer(x), Real(y)) => Ok(Real(*x as f64 - y)),
        (Real(x), BigInteger(y)) => Ok(Real(x - y.to_f64().unwrap_or(f64::INFINITY))),
        (BigInteger(x), Real(y)) => Ok(Real(x.to_f64().unwrap_or(f64::INFINITY) - y)),
        (Real(x), Rational(y)) => Ok(Real(x - y.to_f64().unwrap_or(f64::INFINITY))),
        (Rational(x), Real(y)) => Ok(Real(x.to_f64().unwrap_or(f64::INFINITY) - y)),

        // Complex combinations
        (Complex(x), Complex(y)) => {
            let (ref xr, ref xi) = **x;
            let (ref yr, ref yi) = **y;
            let real = sub_impl(xr, yr)?;
            let imag = sub_impl(xi, yi)?;
            Ok(simplify_complex(real, imag))
        }
        (Complex(x), other) if other.is_number() => {
            let (ref xr, ref xi) = **x;
            let real = sub_impl(xr, other)?;
            Ok(simplify_complex(real, xi.clone()))
        }
        (other, Complex(y)) if other.is_number() => {
            let (ref yr, ref yi) = **y;
            let real = sub_impl(other, yr)?;
            let imag = yi.numeric_neg()?;
            Ok(simplify_complex(real, imag))
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

/// Multiply implementation without exactness contagion
fn mul_impl(a: &Value, b: &Value) -> Result<Value, NumericError> {
    use Value::*;

    match (a, b) {
        // Integer * Integer (with overflow check)
        (Integer(x), Integer(y)) => match x.checked_mul(*y) {
            Some(prod) => Ok(Integer(prod)),
            None => Ok(BigInteger(BigInt::from(*x) * BigInt::from(*y))),
        },

        // BigInteger combinations
        (BigInteger(x), Integer(y)) | (Integer(y), BigInteger(x)) => {
            Ok(BigInteger(x.clone() * BigInt::from(*y)))
        }
        (BigInteger(x), BigInteger(y)) => Ok(BigInteger(x.clone() * y.clone())),

        // Rational combinations
        (Rational(x), Rational(y)) => Ok(rational_to_value(x.clone() * y.clone())),
        (Rational(x), Integer(y)) | (Integer(y), Rational(x)) => Ok(rational_to_value(
            x.clone() * BigRational::from(BigInt::from(*y)),
        )),
        (Rational(x), BigInteger(y)) | (BigInteger(y), Rational(x)) => {
            Ok(rational_to_value(x.clone() * BigRational::from(y.clone())))
        }

        // Real combinations (inexact)
        (Real(x), Real(y)) => Ok(Real(x * y)),
        (Real(x), Integer(y)) | (Integer(y), Real(x)) => Ok(Real(x * *y as f64)),
        (Real(x), BigInteger(y)) | (BigInteger(y), Real(x)) => {
            Ok(Real(x * y.to_f64().unwrap_or(f64::INFINITY)))
        }
        (Real(x), Rational(y)) | (Rational(y), Real(x)) => {
            Ok(Real(x * y.to_f64().unwrap_or(f64::INFINITY)))
        }

        // Complex multiplication: (a+bi)(c+di) = (ac-bd) + (ad+bc)i
        (Complex(x), Complex(y)) => {
            let (ref a, ref b_val) = **x;
            let (ref c, ref d) = **y;
            // real = ac - bd
            let ac = mul_impl(a, c)?;
            let bd = mul_impl(b_val, d)?;
            let real = sub_impl(&ac, &bd)?;
            // imag = ad + bc
            let ad = mul_impl(a, d)?;
            let bc = mul_impl(b_val, c)?;
            let imag = add_impl(&ad, &bc)?;
            Ok(simplify_complex(real, imag))
        }
        (Complex(x), other) | (other, Complex(x)) if other.is_number() => {
            let (ref xr, ref xi) = **x;
            let real = mul_impl(xr, other)?;
            let imag = mul_impl(xi, other)?;
            Ok(simplify_complex(real, imag))
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

/// Divide implementation without exactness contagion
fn div_impl(a: &Value, b: &Value) -> Result<Value, NumericError> {
    use Value::*;

    match (a, b) {
        // Integer / Integer -> Rational (exact division)
        (Integer(x), Integer(y)) => Ok(rational_to_value(BigRational::new(
            BigInt::from(*x),
            BigInt::from(*y),
        ))),

        // BigInteger combinations
        (BigInteger(x), Integer(y)) => Ok(rational_to_value(BigRational::new(
            x.clone(),
            BigInt::from(*y),
        ))),
        (Integer(x), BigInteger(y)) => Ok(rational_to_value(BigRational::new(
            BigInt::from(*x),
            y.clone(),
        ))),
        (BigInteger(x), BigInteger(y)) => {
            Ok(rational_to_value(BigRational::new(x.clone(), y.clone())))
        }

        // Rational combinations
        (Rational(x), Rational(y)) => Ok(rational_to_value(x.clone() / y.clone())),
        (Rational(x), Integer(y)) => Ok(rational_to_value(
            x.clone() / BigRational::from(BigInt::from(*y)),
        )),
        (Integer(x), Rational(y)) => Ok(rational_to_value(
            BigRational::from(BigInt::from(*x)) / y.clone(),
        )),
        (Rational(x), BigInteger(y)) => {
            Ok(rational_to_value(x.clone() / BigRational::from(y.clone())))
        }
        (BigInteger(x), Rational(y)) => {
            Ok(rational_to_value(BigRational::from(x.clone()) / y.clone()))
        }

        // Real combinations (inexact)
        (Real(x), Real(y)) => Ok(Real(x / y)),
        (Real(x), Integer(y)) => Ok(Real(x / *y as f64)),
        (Integer(x), Real(y)) => Ok(Real(*x as f64 / y)),
        (Real(x), BigInteger(y)) => Ok(Real(x / y.to_f64().unwrap_or(f64::INFINITY))),
        (BigInteger(x), Real(y)) => Ok(Real(x.to_f64().unwrap_or(f64::INFINITY) / y)),
        (Real(x), Rational(y)) => Ok(Real(x / y.to_f64().unwrap_or(f64::INFINITY))),
        (Rational(x), Real(y)) => Ok(Real(x.to_f64().unwrap_or(f64::INFINITY) / y)),

        // Complex division: (a+bi)/(c+di) = (ac+bd)/(c²+d²) + (bc-ad)/(c²+d²)i
        (Complex(x), Complex(y)) => {
            let (ref a, ref b_val) = **x;
            let (ref c, ref d) = **y;
            // denom = c² + d²
            let c2 = mul_impl(c, c)?;
            let d2 = mul_impl(d, d)?;
            let denom = add_impl(&c2, &d2)?;
            // real = (ac + bd) / denom
            let ac = mul_impl(a, c)?;
            let bd = mul_impl(b_val, d)?;
            let num_real = add_impl(&ac, &bd)?;
            let real = div_impl(&num_real, &denom)?;
            // imag = (bc - ad) / denom
            let bc = mul_impl(b_val, c)?;
            let ad = mul_impl(a, d)?;
            let num_imag = sub_impl(&bc, &ad)?;
            let imag = div_impl(&num_imag, &denom)?;
            Ok(simplify_complex(real, imag))
        }
        (Complex(x), other) if other.is_number() => {
            let (ref xr, ref xi) = **x;
            let real = div_impl(xr, other)?;
            let imag = div_impl(xi, other)?;
            Ok(simplify_complex(real, imag))
        }
        (other, Complex(y)) if other.is_number() => {
            // a / (c+di) = a(c-di) / (c²+d²)
            let (ref c, ref d) = **y;
            let c2 = mul_impl(c, c)?;
            let d2 = mul_impl(d, d)?;
            let denom = add_impl(&c2, &d2)?;
            let ac = mul_impl(other, c)?;
            let ad = mul_impl(other, d)?;
            let neg_ad = ad.numeric_neg()?;
            let real = div_impl(&ac, &denom)?;
            let imag = div_impl(&neg_ad, &denom)?;
            Ok(simplify_complex(real, imag))
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

// =============================================================================
// Helper functions
// =============================================================================

/// Convert a BigRational to the simplest Value representation.
/// If it's an integer, return Integer or BigInteger.
pub(crate) fn rational_to_value(r: BigRational) -> Value {
    if r.is_integer() {
        let n = r.numer().clone();
        if let Some(i) = n.to_i64() {
            return Value::Integer(i);
        }
        return Value::BigInteger(n);
    }
    Value::Rational(r)
}

/// Simplify a complex number: if imaginary is zero, return just real.
pub(crate) fn simplify_complex(real: Value, imag: Value) -> Value {
    use super::predicates::is_exact_zero;

    if is_exact_zero(&imag) {
        return real;
    }

    // Also simplify if inexact zero (but preserve the inexactness of real part)
    if let Value::Real(f) = &imag {
        if *f == 0.0 {
            // Keep as complex only if real part is also inexact
            if real.is_inexact() {
                return real;
            }
            // If real is exact but imag is inexact zero, result should be inexact real
            return real.to_inexact();
        }
    }

    Value::Complex(Box::new((real, imag)))
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

    fn assert_real(v: &Value, expected: f64) {
        match v {
            Value::Real(f) => assert!((f - expected).abs() < 1e-10),
            _ => panic!("Expected Real, got {:?}", v),
        }
    }

    #[test]
    fn test_add_integers() {
        let a = Value::Integer(2);
        let b = Value::Integer(3);
        assert_integer(&a.numeric_add(&b).unwrap(), 5);
    }

    #[test]
    fn test_add_overflow() {
        let a = Value::Integer(i64::MAX);
        let b = Value::Integer(1);
        let result = a.numeric_add(&b).unwrap();
        match result {
            Value::BigInteger(_) => {}
            _ => panic!("Expected BigInteger on overflow"),
        }
    }

    #[test]
    fn test_add_mixed_exactness() {
        let a = Value::Integer(2);
        let b = Value::Real(3.0);
        let result = a.numeric_add(&b).unwrap();
        assert_real(&result, 5.0);
    }

    #[test]
    fn test_add_nan_propagation() {
        let a = Value::Integer(2);
        let b = Value::Real(f64::NAN);
        let result = a.numeric_add(&b).unwrap();
        assert!(result.is_nan());
    }

    #[test]
    fn test_sub_integers() {
        let a = Value::Integer(5);
        let b = Value::Integer(3);
        assert_integer(&a.numeric_sub(&b).unwrap(), 2);
    }

    #[test]
    fn test_mul_integers() {
        let a = Value::Integer(3);
        let b = Value::Integer(4);
        assert_integer(&a.numeric_mul(&b).unwrap(), 12);
    }

    #[test]
    fn test_div_exact() {
        let a = Value::Integer(6);
        let b = Value::Integer(4);
        let result = a.numeric_div(&b).unwrap();
        // 6/4 = 3/2 as rational
        match result {
            Value::Rational(r) => {
                assert_eq!(r, BigRational::new(3.into(), 2.into()));
            }
            _ => panic!("Expected Rational, got {:?}", result),
        }
    }

    #[test]
    fn test_div_exact_integer_result() {
        let a = Value::Integer(6);
        let b = Value::Integer(2);
        let result = a.numeric_div(&b).unwrap();
        // 6/2 = 3, should simplify to Integer
        assert_integer(&result, 3);
    }

    #[test]
    fn test_div_by_zero_exact() {
        let a = Value::Integer(5);
        let b = Value::Integer(0);
        assert!(matches!(
            a.numeric_div(&b),
            Err(NumericError::DivisionByZero)
        ));
    }

    #[test]
    fn test_div_by_zero_inexact() {
        let a = Value::Real(5.0);
        let b = Value::Real(0.0);
        let result = a.numeric_div(&b).unwrap();
        assert!(result.is_infinite());
    }

    #[test]
    fn test_neg_integer() {
        assert_integer(&Value::Integer(5).numeric_neg().unwrap(), -5);
        assert_integer(&Value::Integer(-5).numeric_neg().unwrap(), 5);
    }

    #[test]
    fn test_abs_integer() {
        assert_integer(&Value::Integer(-5).numeric_abs().unwrap(), 5);
        assert_integer(&Value::Integer(5).numeric_abs().unwrap(), 5);
    }

    #[test]
    fn test_complex_addition() {
        let a = Value::Complex(Box::new((Value::Integer(1), Value::Integer(2))));
        let b = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        let result = a.numeric_add(&b).unwrap();
        match result {
            Value::Complex(parts) => {
                let (ref r, ref i) = *parts;
                assert_integer(r, 4);
                assert_integer(i, 6);
            }
            _ => panic!("Expected Complex"),
        }
    }

    #[test]
    fn test_complex_multiplication() {
        // (1+2i)(3+4i) = 3+4i+6i+8i² = 3+10i-8 = -5+10i
        let a = Value::Complex(Box::new((Value::Integer(1), Value::Integer(2))));
        let b = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        let result = a.numeric_mul(&b).unwrap();
        match result {
            Value::Complex(parts) => {
                let (ref r, ref i) = *parts;
                assert_integer(r, -5);
                assert_integer(i, 10);
            }
            _ => panic!("Expected Complex"),
        }
    }

    #[test]
    fn test_simplify_complex_zero_imag() {
        // (3+0i) should simplify to 3
        let c = simplify_complex(Value::Integer(3), Value::Integer(0));
        assert_integer(&c, 3);
    }
}
