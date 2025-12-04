//! Integer division operations: quotient, remainder, modulo, gcd, lcm
//!
//! R7RS Section 6.2.6
//!
//! Also includes numerator and denominator for rationals.

use super::error::NumericError;
use super::exactness::{apply_contagion, result_exactness, result_exactness_many};
use super::rounding::bigint_to_value;
use crate::value::Value;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

impl Value {
    /// Quotient: integer division truncated toward zero
    ///
    /// (quotient 13 4) => 3
    /// (quotient -13 4) => -3
    /// Preserves inexactness from operands.
    pub fn numeric_quotient(&self, other: &Value) -> Result<Value, NumericError> {
        let exactness = result_exactness(self, other);
        let a = self.to_bigint()?;
        let b = other.to_bigint()?;

        if b.is_zero() {
            return Err(NumericError::DivisionByZero);
        }

        // Integer division truncates toward zero
        let q = &a / &b;
        Ok(apply_contagion(bigint_to_value(q), exactness))
    }

    /// Remainder: remainder from quotient
    ///
    /// Sign of result matches sign of dividend.
    /// (remainder 13 4) => 1
    /// (remainder -13 4) => -1
    /// Preserves inexactness from operands.
    pub fn numeric_remainder(&self, other: &Value) -> Result<Value, NumericError> {
        let exactness = result_exactness(self, other);
        let a = self.to_bigint()?;
        let b = other.to_bigint()?;

        if b.is_zero() {
            return Err(NumericError::DivisionByZero);
        }

        let r = &a % &b;
        Ok(apply_contagion(bigint_to_value(r), exactness))
    }

    /// Modulo: modular arithmetic
    ///
    /// Sign of result matches sign of divisor.
    /// (modulo 13 4) => 1
    /// (modulo -13 4) => 3
    /// Preserves inexactness from operands.
    pub fn numeric_modulo(&self, other: &Value) -> Result<Value, NumericError> {
        let exactness = result_exactness(self, other);
        let a = self.to_bigint()?;
        let b = other.to_bigint()?;

        if b.is_zero() {
            return Err(NumericError::DivisionByZero);
        }

        // modulo result has same sign as divisor
        let r = &a % &b;
        let result = if r.is_zero() || r.sign() == b.sign() {
            r
        } else {
            r + &b
        };

        Ok(apply_contagion(bigint_to_value(result), exactness))
    }

    /// Floor quotient: floor(a/b)
    ///
    /// (floor-quotient 5 2) => 2
    /// (floor-quotient -5 2) => -3
    /// Preserves inexactness from operands.
    pub fn numeric_floor_quotient(&self, other: &Value) -> Result<Value, NumericError> {
        let exactness = result_exactness(self, other);
        let a = self.to_bigint()?;
        let b = other.to_bigint()?;

        if b.is_zero() {
            return Err(NumericError::DivisionByZero);
        }

        let q = floor_div(&a, &b);
        Ok(apply_contagion(bigint_to_value(q), exactness))
    }

    /// Floor remainder
    ///
    /// (floor-remainder 5 2) => 1
    /// (floor-remainder -5 2) => 1
    /// Preserves inexactness from operands.
    pub fn numeric_floor_remainder(&self, other: &Value) -> Result<Value, NumericError> {
        let exactness = result_exactness(self, other);
        let a = self.to_bigint()?;
        let b = other.to_bigint()?;

        if b.is_zero() {
            return Err(NumericError::DivisionByZero);
        }

        let q = floor_div(&a, &b);
        let r = a - q * &b;
        Ok(apply_contagion(bigint_to_value(r), exactness))
    }

    /// Truncate quotient: round toward zero
    ///
    /// Same as quotient.
    pub fn numeric_truncate_quotient(&self, other: &Value) -> Result<Value, NumericError> {
        self.numeric_quotient(other)
    }

    /// Truncate remainder
    ///
    /// Same as remainder.
    pub fn numeric_truncate_remainder(&self, other: &Value) -> Result<Value, NumericError> {
        self.numeric_remainder(other)
    }

    /// Get numerator of a rational number
    ///
    /// For integers, returns the integer itself.
    pub fn numerator(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(n) => Ok(Value::Integer(*n)),
            Value::BigInteger(n) => Ok(bigint_to_value(n.clone())),
            Value::Rational(r) => Ok(bigint_to_value(r.numer().clone())),
            Value::Real(f) => {
                // Convert to rational first, then get numerator
                let r = float_to_rational(*f)?;
                Ok(Value::Real(r.numer().to_f64().unwrap_or(f64::INFINITY)))
            }
            other => Err(NumericError::NotReal {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Get denominator of a rational number
    ///
    /// For integers, returns 1.
    pub fn denominator(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(_) => Ok(Value::Integer(1)),
            Value::BigInteger(_) => Ok(Value::Integer(1)),
            Value::Rational(r) => Ok(bigint_to_value(r.denom().clone())),
            Value::Real(f) => {
                // Convert to rational first, then get denominator
                let r = float_to_rational(*f)?;
                Ok(Value::Real(r.denom().to_f64().unwrap_or(f64::INFINITY)))
            }
            other => Err(NumericError::NotReal {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Convert to BigInt if possible
    fn to_bigint(&self) -> Result<BigInt, NumericError> {
        match self {
            Value::Integer(n) => Ok(BigInt::from(*n)),
            Value::BigInteger(n) => Ok(n.clone()),
            Value::Rational(r) if r.is_integer() => Ok(r.numer().clone()),
            Value::Real(f) if f.fract() == 0.0 && f.is_finite() => Ok(BigInt::from(*f as i64)),
            _ => Err(NumericError::NotInteger {
                got: self.type_name().to_string(),
            }),
        }
    }
}

/// GCD of two numbers
///
/// Always returns non-negative result.
/// Preserves inexactness from operands.
pub fn numeric_gcd(a: &Value, b: &Value) -> Result<Value, NumericError> {
    let exactness = result_exactness(a, b);
    let ai = value_to_bigint(a)?;
    let bi = value_to_bigint(b)?;

    let result = gcd_bigint(&ai, &bi);
    Ok(apply_contagion(bigint_to_value(result), exactness))
}

/// LCM of two numbers
///
/// Always returns non-negative result.
/// Preserves inexactness from operands.
pub fn numeric_lcm(a: &Value, b: &Value) -> Result<Value, NumericError> {
    let exactness = result_exactness(a, b);
    let ai = value_to_bigint(a)?;
    let bi = value_to_bigint(b)?;

    if ai.is_zero() || bi.is_zero() {
        return Ok(apply_contagion(Value::Integer(0), exactness));
    }

    let g = gcd_bigint(&ai, &bi);
    let result = (ai.abs() / g) * bi.abs();
    Ok(apply_contagion(bigint_to_value(result), exactness))
}

/// GCD for multiple values (variadic)
/// Preserves inexactness from operands.
pub fn numeric_gcd_many(values: &[Value]) -> Result<Value, NumericError> {
    if values.is_empty() {
        return Ok(Value::Integer(0));
    }

    let exactness = result_exactness_many(values);
    let mut result = value_to_bigint(&values[0])?;
    for v in &values[1..] {
        let bi = value_to_bigint(v)?;
        result = gcd_bigint(&result, &bi);
    }

    Ok(apply_contagion(bigint_to_value(result), exactness))
}

/// LCM for multiple values (variadic)
/// Preserves inexactness from operands.
pub fn numeric_lcm_many(values: &[Value]) -> Result<Value, NumericError> {
    if values.is_empty() {
        return Ok(Value::Integer(1));
    }

    let exactness = result_exactness_many(values);
    let mut result = value_to_bigint(&values[0])?.abs();
    for v in &values[1..] {
        let bi = value_to_bigint(v)?.abs();
        if result.is_zero() || bi.is_zero() {
            return Ok(apply_contagion(Value::Integer(0), exactness));
        }
        let g = gcd_bigint(&result, &bi);
        result = (&result / &g) * &bi;
    }

    Ok(apply_contagion(bigint_to_value(result), exactness))
}

// =============================================================================
// Helper functions
// =============================================================================

/// Convert Value to BigInt
fn value_to_bigint(v: &Value) -> Result<BigInt, NumericError> {
    match v {
        Value::Integer(n) => Ok(BigInt::from(*n)),
        Value::BigInteger(n) => Ok(n.clone()),
        Value::Rational(r) if r.is_integer() => Ok(r.numer().clone()),
        Value::Real(f) if f.fract() == 0.0 && f.is_finite() => Ok(BigInt::from(*f as i64)),
        _ => Err(NumericError::NotInteger {
            got: v.type_name().to_string(),
        }),
    }
}

/// GCD for BigInt using Euclidean algorithm
fn gcd_bigint(a: &BigInt, b: &BigInt) -> BigInt {
    let mut a = a.abs();
    let mut b = b.abs();

    while !b.is_zero() {
        let t = b.clone();
        b = &a % &b;
        a = t;
    }

    a
}

/// Floor division for BigInt
fn floor_div(a: &BigInt, b: &BigInt) -> BigInt {
    let q = a / b;
    let r = a % b;

    // Adjust if remainder has opposite sign to divisor
    if !r.is_zero() && r.sign() != b.sign() {
        q - 1
    } else {
        q
    }
}

/// Convert float to rational (for numerator/denominator)
fn float_to_rational(f: f64) -> Result<BigRational, NumericError> {
    if f.is_nan() || f.is_infinite() {
        return Err(NumericError::NoExactRepresentation {
            value: format!("{}", f),
        });
    }

    // Simple conversion using continued fractions approximation
    // For exact representation, we'd need to analyze the float's bits
    let (mantissa, exponent, sign) = decode_float(f);

    let two = BigInt::from(2);
    let numer = BigInt::from(mantissa);

    let rational = if exponent >= 0 {
        let scale = two.pow(exponent as u32);
        BigRational::new(numer * scale, BigInt::one())
    } else {
        let scale = two.pow((-exponent) as u32);
        BigRational::new(numer, scale)
    };

    if sign < 0 {
        Ok(-rational)
    } else {
        Ok(rational)
    }
}

/// Decode float into mantissa, exponent, and sign
fn decode_float(f: f64) -> (u64, i32, i32) {
    if f == 0.0 {
        return (0, 0, if f.is_sign_negative() { -1 } else { 1 });
    }

    let bits = f.to_bits();
    let sign = if (bits >> 63) != 0 { -1 } else { 1 };
    let exponent_bits = ((bits >> 52) & 0x7FF) as i32;
    let mantissa_bits = bits & 0x000F_FFFF_FFFF_FFFF;

    if exponent_bits == 0 {
        // Subnormal
        (mantissa_bits, -1074, sign)
    } else {
        // Normal: implicit leading 1
        let mantissa = mantissa_bits | 0x0010_0000_0000_0000;
        let exponent = exponent_bits - 1023 - 52;
        (mantissa, exponent, sign)
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
    fn test_quotient() {
        assert_integer(
            &Value::Integer(13)
                .numeric_quotient(&Value::Integer(4))
                .unwrap(),
            3,
        );
        assert_integer(
            &Value::Integer(-13)
                .numeric_quotient(&Value::Integer(4))
                .unwrap(),
            -3,
        );
        assert_integer(
            &Value::Integer(13)
                .numeric_quotient(&Value::Integer(-4))
                .unwrap(),
            -3,
        );
    }

    #[test]
    fn test_remainder() {
        assert_integer(
            &Value::Integer(13)
                .numeric_remainder(&Value::Integer(4))
                .unwrap(),
            1,
        );
        assert_integer(
            &Value::Integer(-13)
                .numeric_remainder(&Value::Integer(4))
                .unwrap(),
            -1,
        );
    }

    #[test]
    fn test_modulo() {
        assert_integer(
            &Value::Integer(13)
                .numeric_modulo(&Value::Integer(4))
                .unwrap(),
            1,
        );
        assert_integer(
            &Value::Integer(-13)
                .numeric_modulo(&Value::Integer(4))
                .unwrap(),
            3,
        );
    }

    #[test]
    fn test_floor_quotient() {
        assert_integer(
            &Value::Integer(5)
                .numeric_floor_quotient(&Value::Integer(2))
                .unwrap(),
            2,
        );
        assert_integer(
            &Value::Integer(-5)
                .numeric_floor_quotient(&Value::Integer(2))
                .unwrap(),
            -3,
        );
    }

    #[test]
    fn test_gcd() {
        assert_integer(
            &numeric_gcd(&Value::Integer(12), &Value::Integer(8)).unwrap(),
            4,
        );
        assert_integer(
            &numeric_gcd(&Value::Integer(-12), &Value::Integer(8)).unwrap(),
            4,
        );
        assert_integer(
            &numeric_gcd(&Value::Integer(0), &Value::Integer(5)).unwrap(),
            5,
        );
    }

    #[test]
    fn test_lcm() {
        assert_integer(
            &numeric_lcm(&Value::Integer(12), &Value::Integer(8)).unwrap(),
            24,
        );
        assert_integer(
            &numeric_lcm(&Value::Integer(0), &Value::Integer(5)).unwrap(),
            0,
        );
    }

    #[test]
    fn test_numerator_denominator() {
        assert_integer(&Value::Integer(5).numerator().unwrap(), 5);
        assert_integer(&Value::Integer(5).denominator().unwrap(), 1);

        let r = BigRational::new(BigInt::from(3), BigInt::from(4));
        assert_integer(&Value::Rational(r.clone()).numerator().unwrap(), 3);
        assert_integer(&Value::Rational(r).denominator().unwrap(), 4);
    }
}
