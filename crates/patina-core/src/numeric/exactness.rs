//! Exactness conversion and contagion rules
//!
//! R7RS Section 6.2.4: Operations that produce inexact results always produce
//! inexact results when given inexact arguments.

use super::error::NumericError;
use crate::value::Value;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::ToPrimitive;

/// Exactness of a numeric result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exactness {
    Exact,
    Inexact,
}

impl Value {
    /// Convert to inexact representation.
    ///
    /// - Integer/BigInteger/Rational → Real
    /// - Complex with exact parts → Complex with inexact parts
    /// - Already inexact → unchanged
    pub fn to_inexact(&self) -> Value {
        match self {
            Value::Integer(n) => Value::Real(*n as f64),
            Value::BigInteger(n) => Value::Real(n.to_f64().unwrap_or(f64::INFINITY)),
            Value::Rational(r) => Value::Real(r.to_f64().unwrap_or(f64::INFINITY)),
            Value::Real(_) => self.clone(),
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                Value::Complex(Box::new((real.to_inexact(), imag.to_inexact())))
            }
            // Non-numeric values return unchanged (will error in calling code)
            _ => self.clone(),
        }
    }

    /// Convert to exact representation.
    ///
    /// - Real → Rational (using exact float representation)
    /// - Complex with inexact parts → Complex with exact parts
    /// - Already exact → unchanged
    ///
    /// Returns error for NaN or infinity (no exact representation).
    pub fn to_exact(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => Ok(self.clone()),
            Value::Real(f) => {
                if f.is_nan() {
                    return Err(NumericError::NoExactRepresentation {
                        value: "+nan.0".to_string(),
                    });
                }
                if f.is_infinite() {
                    return Err(NumericError::NoExactRepresentation {
                        value: if *f > 0.0 {
                            "+inf.0".to_string()
                        } else {
                            "-inf.0".to_string()
                        },
                    });
                }
                // Convert float to rational
                Ok(float_to_exact(*f))
            }
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                let exact_real = real.to_exact()?;
                let exact_imag = imag.to_exact()?;
                Ok(Value::Complex(Box::new((exact_real, exact_imag))))
            }
            other => Err(NumericError::NotANumber {
                got: other.type_name().to_string(),
            }),
        }
    }
}

/// Convert a float to an exact rational representation.
///
/// Uses the property that any finite f64 can be exactly represented as a
/// rational with a power-of-2 denominator.
fn float_to_exact(f: f64) -> Value {
    // Handle integer case
    if f.trunc() == f && f.abs() < i64::MAX as f64 {
        return Value::Integer(f as i64);
    }

    // For other floats, convert via rational
    // BigRational::from_f64 gives exact representation
    if let Some(r) = BigRational::from_float(f) {
        // Check if it's actually an integer
        if r.is_integer() {
            let n = r.numer().clone();
            if let Some(i) = n.to_i64() {
                return Value::Integer(i);
            }
            return Value::BigInteger(n);
        }
        return Value::Rational(r);
    }

    // Fallback: approximate with simple fraction
    // This shouldn't happen for normal floats
    Value::Rational(BigRational::new(
        BigInt::from((f * 1_000_000.0) as i64),
        BigInt::from(1_000_000),
    ))
}

/// Determine the result exactness for a binary operation.
///
/// R7RS Rule: If any operand is inexact, result is inexact.
#[inline]
pub fn result_exactness(a: &Value, b: &Value) -> Exactness {
    if a.is_inexact() || b.is_inexact() {
        Exactness::Inexact
    } else {
        Exactness::Exact
    }
}

/// Determine the result exactness for multiple operands.
#[inline]
pub fn result_exactness_many(values: &[Value]) -> Exactness {
    if values.iter().any(|v| v.is_inexact()) {
        Exactness::Inexact
    } else {
        Exactness::Exact
    }
}

/// Apply inexact contagion: convert to inexact if required.
#[inline]
pub fn apply_contagion(value: Value, exactness: Exactness) -> Value {
    match exactness {
        Exactness::Exact => value,
        Exactness::Inexact => {
            if value.is_exact() {
                value.to_inexact()
            } else {
                value
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_rational::BigRational;

    fn assert_real(v: &Value, expected: f64) {
        match v {
            Value::Real(f) => {
                if expected.is_nan() {
                    assert!(f.is_nan());
                } else {
                    assert!(
                        (f - expected).abs() < 1e-10,
                        "expected {} got {}",
                        expected,
                        f
                    );
                }
            }
            _ => panic!("Expected Real, got {:?}", v),
        }
    }

    fn assert_integer(v: &Value, expected: i64) {
        match v {
            Value::Integer(n) => assert_eq!(*n, expected),
            _ => panic!("Expected Integer, got {:?}", v),
        }
    }

    #[test]
    fn test_to_inexact() {
        assert_real(&Value::Integer(42).to_inexact(), 42.0);
        assert_real(
            &Value::Rational(BigRational::new(1.into(), 2.into())).to_inexact(),
            0.5,
        );
        assert_real(&Value::Real(2.75).to_inexact(), 2.75);
    }

    #[test]
    fn test_to_exact() {
        // Integer already exact
        assert_integer(&Value::Integer(42).to_exact().unwrap(), 42);

        // Float that's an integer
        assert_integer(&Value::Real(42.0).to_exact().unwrap(), 42);

        // Float with fractional part
        let result = Value::Real(0.5).to_exact().unwrap();
        match result {
            Value::Rational(r) => {
                assert_eq!(r, BigRational::new(1.into(), 2.into()));
            }
            _ => panic!("Expected Rational"),
        }

        // NaN has no exact representation
        assert!(Value::Real(f64::NAN).to_exact().is_err());

        // Infinity has no exact representation
        assert!(Value::Real(f64::INFINITY).to_exact().is_err());
    }

    #[test]
    fn test_result_exactness() {
        assert_eq!(
            result_exactness(&Value::Integer(1), &Value::Integer(2)),
            Exactness::Exact
        );
        assert_eq!(
            result_exactness(&Value::Integer(1), &Value::Real(2.0)),
            Exactness::Inexact
        );
        assert_eq!(
            result_exactness(&Value::Real(1.0), &Value::Real(2.0)),
            Exactness::Inexact
        );
    }

    #[test]
    fn test_apply_contagion() {
        // Exact value stays exact when exactness is Exact
        assert_integer(&apply_contagion(Value::Integer(42), Exactness::Exact), 42);

        // Exact value becomes inexact when exactness is Inexact
        assert_real(
            &apply_contagion(Value::Integer(42), Exactness::Inexact),
            42.0,
        );

        // Inexact value stays inexact regardless
        assert_real(&apply_contagion(Value::Real(42.0), Exactness::Exact), 42.0);
    }

    #[test]
    fn test_complex_exactness_conversion() {
        // Exact complex to inexact
        let exact_c = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        let inexact_c = exact_c.to_inexact();
        match inexact_c {
            Value::Complex(parts) => {
                let (ref r, ref i) = *parts;
                assert_real(r, 3.0);
                assert_real(i, 4.0);
            }
            _ => panic!("Expected Complex"),
        }

        // Inexact complex to exact
        let inexact_c = Value::Complex(Box::new((Value::Real(3.0), Value::Real(4.0))));
        let exact_c = inexact_c.to_exact().unwrap();
        match exact_c {
            Value::Complex(parts) => {
                let (ref r, ref i) = *parts;
                assert_integer(r, 3);
                assert_integer(i, 4);
            }
            _ => panic!("Expected Complex"),
        }
    }
}
