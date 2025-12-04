//! Complex number operations: real-part, imag-part, magnitude, angle,
//! make-rectangular, make-polar
//!
//! R7RS Section 6.2.6

use super::error::NumericError;
use super::predicates::is_exact_zero;
use crate::value::Value;
use num_traits::Signed;

impl Value {
    /// Extract real part of a number.
    ///
    /// For real numbers, returns the number itself.
    /// For complex, returns the real component preserving exactness.
    pub fn real_part(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) | Value::Real(_) => {
                Ok(self.clone())
            }
            Value::Complex(parts) => {
                let (ref real, _) = **parts;
                Ok(real.clone())
            }
            other => Err(NumericError::NotANumber {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Extract imaginary part of a number.
    ///
    /// For real numbers, returns exact 0.
    /// For complex, returns the imaginary component preserving exactness.
    pub fn imag_part(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => Ok(Value::Integer(0)),
            Value::Real(_) => {
                // Inexact real has inexact zero imaginary part
                Ok(Value::Integer(0))
            }
            Value::Complex(parts) => {
                let (_, ref imag) = **parts;
                Ok(imag.clone())
            }
            other => Err(NumericError::NotANumber {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Compute magnitude (|z|) of a number.
    ///
    /// For real numbers, returns absolute value (always inexact per R7RS).
    /// For complex, returns sqrt(real² + imag²) (always inexact).
    pub fn magnitude(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(n) => {
                // R7RS: magnitude always returns inexact result
                Ok(Value::Real((*n as f64).abs()))
            }
            Value::BigInteger(n) => {
                use num_traits::ToPrimitive;
                Ok(Value::Real(n.to_f64().unwrap_or(f64::INFINITY).abs()))
            }
            Value::Rational(r) => {
                use num_traits::ToPrimitive;
                Ok(Value::Real(r.to_f64().unwrap_or(f64::INFINITY).abs()))
            }
            Value::Real(f) => Ok(Value::Real(f.abs())),
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                // |z| = sqrt(real² + imag²)
                let r = real.to_f64()?;
                let i = imag.to_f64()?;
                Ok(Value::Real((r * r + i * i).sqrt()))
            }
            other => Err(NumericError::NotANumber {
                got: other.type_name().to_string(),
            }),
        }
    }

    /// Compute angle (argument) of a number in radians.
    ///
    /// For positive real, returns 0.
    /// For negative real, returns π.
    /// For complex, returns atan2(imag, real).
    pub fn angle(&self) -> Result<Value, NumericError> {
        match self {
            Value::Integer(n) => {
                if *n >= 0 {
                    Ok(Value::Real(0.0))
                } else {
                    Ok(Value::Real(std::f64::consts::PI))
                }
            }
            Value::BigInteger(n) => {
                use num_bigint::Sign;
                if n.sign() != Sign::Minus {
                    Ok(Value::Real(0.0))
                } else {
                    Ok(Value::Real(std::f64::consts::PI))
                }
            }
            Value::Rational(r) => {
                use num_traits::Signed;
                if !r.is_negative() {
                    Ok(Value::Real(0.0))
                } else {
                    Ok(Value::Real(std::f64::consts::PI))
                }
            }
            Value::Real(f) => {
                if *f >= 0.0 {
                    Ok(Value::Real(0.0))
                } else {
                    Ok(Value::Real(std::f64::consts::PI))
                }
            }
            Value::Complex(parts) => {
                let (ref real, ref imag) = **parts;
                let r = real.to_f64()?;
                let i = imag.to_f64()?;
                Ok(Value::Real(i.atan2(r)))
            }
            other => Err(NumericError::NotANumber {
                got: other.type_name().to_string(),
            }),
        }
    }
}

/// Construct a complex number from rectangular coordinates.
///
/// If imaginary is exact zero, returns just the real part.
pub fn make_rectangular(real: &Value, imag: &Value) -> Result<Value, NumericError> {
    // Validate inputs
    if !real.is_number() {
        return Err(NumericError::NotANumber {
            got: real.type_name().to_string(),
        });
    }
    if !imag.is_number() {
        return Err(NumericError::NotANumber {
            got: imag.type_name().to_string(),
        });
    }

    // Complex components must be real, not complex
    if matches!(real, Value::Complex(_)) {
        return Err(NumericError::NotReal {
            got: "complex".to_string(),
        });
    }
    if matches!(imag, Value::Complex(_)) {
        return Err(NumericError::NotReal {
            got: "complex".to_string(),
        });
    }

    // Simplify if imaginary is exact zero
    if is_exact_zero(imag) {
        return Ok(real.clone());
    }

    // If imaginary is inexact zero, keep as real but apply contagion
    if let Value::Real(f) = imag {
        if *f == 0.0 {
            return Ok(real.to_inexact());
        }
    }

    Ok(Value::Complex(Box::new((real.clone(), imag.clone()))))
}

/// Construct a complex number from polar coordinates.
///
/// magnitude @ angle = magnitude * (cos(angle) + i*sin(angle))
pub fn make_polar(magnitude: &Value, angle: &Value) -> Result<Value, NumericError> {
    // Validate inputs
    if !magnitude.is_number() {
        return Err(NumericError::NotANumber {
            got: magnitude.type_name().to_string(),
        });
    }
    if !angle.is_number() {
        return Err(NumericError::NotANumber {
            got: angle.type_name().to_string(),
        });
    }

    // Components must be real
    if matches!(magnitude, Value::Complex(_)) {
        return Err(NumericError::NotReal {
            got: "complex".to_string(),
        });
    }
    if matches!(angle, Value::Complex(_)) {
        return Err(NumericError::NotReal {
            got: "complex".to_string(),
        });
    }

    let r = magnitude.to_f64()?;
    let theta = angle.to_f64()?;

    let real = r * theta.cos();
    let imag = r * theta.sin();

    // Polar always produces inexact results (due to trig functions)
    if imag.abs() < 1e-15 {
        // Very small imaginary part, treat as real
        Ok(Value::Real(real))
    } else {
        Ok(Value::Complex(Box::new((
            Value::Real(real),
            Value::Real(imag),
        ))))
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
    fn test_real_part() {
        assert_integer(&Value::Integer(5).real_part().unwrap(), 5);
        assert_real(&Value::Real(2.75).real_part().unwrap(), 2.75);

        let c = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        assert_integer(&c.real_part().unwrap(), 3);
    }

    #[test]
    fn test_imag_part() {
        assert_integer(&Value::Integer(5).imag_part().unwrap(), 0);

        let c = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        assert_integer(&c.imag_part().unwrap(), 4);
    }

    #[test]
    fn test_magnitude() {
        // magnitude always returns inexact
        assert_real(&Value::Integer(5).magnitude().unwrap(), 5.0);
        assert_real(&Value::Integer(-5).magnitude().unwrap(), 5.0);

        // |3+4i| = 5
        let c = Value::Complex(Box::new((Value::Integer(3), Value::Integer(4))));
        assert_real(&c.magnitude().unwrap(), 5.0);
    }

    #[test]
    fn test_angle() {
        assert_real(&Value::Integer(5).angle().unwrap(), 0.0);
        assert_real(&Value::Integer(-5).angle().unwrap(), std::f64::consts::PI);

        // angle of i is π/2
        let i = Value::Complex(Box::new((Value::Integer(0), Value::Integer(1))));
        let angle = i.angle().unwrap();
        if let Value::Real(a) = angle {
            assert!((a - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
        } else {
            panic!("Expected Real");
        }
    }

    #[test]
    fn test_make_rectangular() {
        // (make-rectangular 3 4) = 3+4i
        let c = make_rectangular(&Value::Integer(3), &Value::Integer(4)).unwrap();
        match c {
            Value::Complex(parts) => {
                let (ref r, ref i) = *parts;
                assert_integer(r, 3);
                assert_integer(i, 4);
            }
            _ => panic!("Expected Complex"),
        }

        // (make-rectangular 5 0) = 5 (simplifies)
        let r = make_rectangular(&Value::Integer(5), &Value::Integer(0)).unwrap();
        assert_integer(&r, 5);
    }

    #[test]
    fn test_make_polar() {
        // (make-polar 1 0) = 1
        let r = make_polar(&Value::Integer(1), &Value::Integer(0)).unwrap();
        if let Value::Real(f) = r {
            assert!((f - 1.0).abs() < 1e-10);
        } else {
            panic!("Expected Real");
        }

        // (make-polar 1 π/2) ≈ i
        let c = make_polar(
            &Value::Integer(1),
            &Value::Real(std::f64::consts::FRAC_PI_2),
        )
        .unwrap();
        match c {
            Value::Complex(parts) => {
                let (Value::Real(r), Value::Real(i)) = *parts else {
                    panic!("Expected Real parts");
                };
                assert!(r.abs() < 1e-10); // Real part ≈ 0
                assert!((i - 1.0).abs() < 1e-10); // Imag part ≈ 1
            }
            _ => panic!("Expected Complex"),
        }
    }
}
