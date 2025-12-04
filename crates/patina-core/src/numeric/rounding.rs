//! Rounding operations: floor, ceiling, truncate, round
//!
//! R7RS Section 6.2.6

use super::error::NumericError;
use crate::value::Value;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};

impl Value {
    /// Floor: largest integer <= x
    ///
    /// For exact inputs, returns exact integer.
    /// For inexact inputs, returns inexact integer.
    pub fn numeric_floor(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(_) | Value::BigInteger(_) => Ok(self.clone()),
            Value::Rational(r) => {
                let floored = floor_rational(r);
                Ok(bigint_to_value(floored))
            }
            Value::Real(f) => {
                if f.is_nan() || f.is_infinite() {
                    Ok(self.clone())
                } else {
                    Ok(Value::Real(f.floor()))
                }
            }
            other => Err(NumericError::NotReal {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Ceiling: smallest integer >= x
    ///
    /// For exact inputs, returns exact integer.
    /// For inexact inputs, returns inexact integer.
    pub fn numeric_ceiling(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(_) | Value::BigInteger(_) => Ok(self.clone()),
            Value::Rational(r) => {
                let ceiled = ceiling_rational(r);
                Ok(bigint_to_value(ceiled))
            }
            Value::Real(f) => {
                if f.is_nan() || f.is_infinite() {
                    Ok(self.clone())
                } else {
                    Ok(Value::Real(f.ceil()))
                }
            }
            other => Err(NumericError::NotReal {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Truncate: round toward zero
    ///
    /// For exact inputs, returns exact integer.
    /// For inexact inputs, returns inexact integer.
    pub fn numeric_truncate(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(_) | Value::BigInteger(_) => Ok(self.clone()),
            Value::Rational(r) => {
                let truncated = truncate_rational(r);
                Ok(bigint_to_value(truncated))
            }
            Value::Real(f) => {
                if f.is_nan() || f.is_infinite() {
                    Ok(self.clone())
                } else {
                    Ok(Value::Real(f.trunc()))
                }
            }
            other => Err(NumericError::NotReal {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Round: round to nearest, ties to even (banker's rounding)
    ///
    /// For exact inputs, returns exact integer.
    /// For inexact inputs, returns inexact integer.
    pub fn numeric_round(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(_) | Value::BigInteger(_) => Ok(self.clone()),
            Value::Rational(r) => {
                let rounded = round_rational(r);
                Ok(bigint_to_value(rounded))
            }
            Value::Real(f) => {
                if f.is_nan() || f.is_infinite() {
                    Ok(self.clone())
                } else {
                    // Rust's round_ties_even is available in nightly
                    // For now, implement banker's rounding manually
                    Ok(Value::Real(bankers_round(*f)))
                }
            }
            other => Err(NumericError::NotReal {
                got: other.type_name().to_string(),
            }),
        }
    }
}

// =============================================================================
// Helper functions
// =============================================================================

/// Floor for BigRational
fn floor_rational(r: &BigRational) -> BigInt {
    if r.is_integer() {
        r.numer().clone()
    } else if r.is_negative() {
        // For negative non-integers, floor is quotient - 1
        r.numer() / r.denom() - 1
    } else {
        // For positive non-integers, floor is quotient
        r.numer() / r.denom()
    }
}

/// Ceiling for BigRational
fn ceiling_rational(r: &BigRational) -> BigInt {
    if r.is_integer() {
        r.numer().clone()
    } else if r.is_positive() {
        // For positive non-integers, ceiling is quotient + 1
        r.numer() / r.denom() + 1
    } else {
        // For negative non-integers, ceiling is quotient
        r.numer() / r.denom()
    }
}

/// Truncate for BigRational (round toward zero)
fn truncate_rational(r: &BigRational) -> BigInt {
    r.numer() / r.denom()
}

/// Round for BigRational (banker's rounding - ties to even)
fn round_rational(r: &BigRational) -> BigInt {
    let floored = floor_rational(r);
    let frac = r - BigRational::from(floored.clone());

    let half = BigRational::new(BigInt::from(1), BigInt::from(2));

    if frac < half {
        floored
    } else if frac > half {
        floored + 1
    } else {
        // Exactly half - round to even
        if &floored % 2 == BigInt::zero() {
            floored
        } else {
            floored + 1
        }
    }
}

/// Banker's rounding for f64
fn bankers_round(x: f64) -> f64 {
    let rounded = x.round();

    // Check if we're exactly at a midpoint (x.5)
    // At a midpoint, x - floor(x) = 0.5 exactly
    let frac = x - x.floor();
    let is_midpoint = (frac - 0.5).abs() < 1e-10;

    if is_midpoint {
        // At midpoint, round to even
        let floor_val = x.floor();
        let ceil_val = x.ceil();

        // Pick the even one
        if floor_val % 2.0 == 0.0 {
            floor_val
        } else {
            ceil_val
        }
    } else {
        rounded
    }
}

/// Convert BigInt to the simplest integer Value
pub fn bigint_to_value(n: BigInt) -> Value {
    if let Some(i) = n.to_i64() {
        Value::Integer(i)
    } else {
        Value::BigInteger(n)
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

    fn assert_real(v: &Value, expected: f64) {
        match v {
            Value::Real(f) => assert!((f - expected).abs() < 1e-10),
            _ => panic!("Expected Real, got {:?}", v),
        }
    }

    #[test]
    fn test_floor() {
        assert_integer(&Value::Integer(5).numeric_floor().unwrap(), 5);
        assert_real(&Value::Real(3.7).numeric_floor().unwrap(), 3.0);
        assert_real(&Value::Real(-3.7).numeric_floor().unwrap(), -4.0);
    }

    #[test]
    fn test_ceiling() {
        assert_integer(&Value::Integer(5).numeric_ceiling().unwrap(), 5);
        assert_real(&Value::Real(3.2).numeric_ceiling().unwrap(), 4.0);
        assert_real(&Value::Real(-3.2).numeric_ceiling().unwrap(), -3.0);
    }

    #[test]
    fn test_truncate() {
        assert_integer(&Value::Integer(5).numeric_truncate().unwrap(), 5);
        assert_real(&Value::Real(3.7).numeric_truncate().unwrap(), 3.0);
        assert_real(&Value::Real(-3.7).numeric_truncate().unwrap(), -3.0);
    }

    #[test]
    fn test_round_bankers() {
        // Banker's rounding: ties go to even
        assert_real(&Value::Real(3.5).numeric_round().unwrap(), 4.0);
        assert_real(&Value::Real(4.5).numeric_round().unwrap(), 4.0); // ties to even
        assert_real(&Value::Real(2.5).numeric_round().unwrap(), 2.0); // ties to even
    }

    #[test]
    fn test_floor_rational() {
        let r = BigRational::new(BigInt::from(7), BigInt::from(2)); // 7/2 = 3.5
        assert_eq!(floor_rational(&r), BigInt::from(3));

        let r = BigRational::new(BigInt::from(-7), BigInt::from(2)); // -7/2 = -3.5
        assert_eq!(floor_rational(&r), BigInt::from(-4));
    }
}
