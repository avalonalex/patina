//! Transcendental functions: sin, cos, tan, asin, acos, atan, exp, log, sqrt, expt
//!
//! R7RS Section 6.2.6
//!
//! These functions generally produce inexact results, except for special cases
//! like (sqrt 4) which could return exact 2.

use super::arithmetic::rational_to_value;
use super::error::NumericError;
use crate::value::Value;
use num_bigint::BigInt;
use num_complex::Complex64;
use num_rational::BigRational;
use num_traits::{Pow, ToPrimitive};

impl Value {
    /// Sine function
    pub fn numeric_sin(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let c = Complex64::new(r.to_f64()?, i.to_f64()?);
                let result = c.sin();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                Ok(Value::Real(x.sin()))
            }
        }
    }

    /// Cosine function
    pub fn numeric_cos(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let c = Complex64::new(r.to_f64()?, i.to_f64()?);
                let result = c.cos();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                Ok(Value::Real(x.cos()))
            }
        }
    }

    /// Tangent function
    pub fn numeric_tan(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let c = Complex64::new(r.to_f64()?, i.to_f64()?);
                let result = c.tan();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                Ok(Value::Real(x.tan()))
            }
        }
    }

    /// Arcsine function
    ///
    /// For |x| > 1, returns complex result.
    pub fn numeric_asin(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let c = Complex64::new(r.to_f64()?, i.to_f64()?);
                let result = c.asin();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                if x.abs() <= 1.0 {
                    Ok(Value::Real(x.asin()))
                } else {
                    // Result is complex
                    let c = Complex64::new(x, 0.0);
                    let result = c.asin();
                    Ok(complex64_to_value(result))
                }
            }
        }
    }

    /// Arccosine function
    ///
    /// For |x| > 1, returns complex result.
    pub fn numeric_acos(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let c = Complex64::new(r.to_f64()?, i.to_f64()?);
                let result = c.acos();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                if x.abs() <= 1.0 {
                    Ok(Value::Real(x.acos()))
                } else {
                    // Result is complex
                    let c = Complex64::new(x, 0.0);
                    let result = c.acos();
                    Ok(complex64_to_value(result))
                }
            }
        }
    }

    /// Arctangent function (one argument)
    pub fn numeric_atan(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let c = Complex64::new(r.to_f64()?, i.to_f64()?);
                let result = c.atan();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                Ok(Value::Real(x.atan()))
            }
        }
    }

    /// Two-argument arctangent: atan2(y, x)
    pub fn numeric_atan2(&self, x: &Value) -> Result<Value, NumericError> {
        let y_val = self.to_f64()?;
        let x_val = x.to_f64()?;
        Ok(Value::Real(y_val.atan2(x_val)))
    }

    /// Exponential function (e^x)
    pub fn numeric_exp(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let c = Complex64::new(r.to_f64()?, i.to_f64()?);
                let result = c.exp();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                Ok(Value::Real(x.exp()))
            }
        }
    }

    /// Natural logarithm
    ///
    /// For negative real numbers, returns complex result.
    pub fn numeric_log(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let c = Complex64::new(r.to_f64()?, i.to_f64()?);
                let result = c.ln();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                if x > 0.0 {
                    Ok(Value::Real(x.ln()))
                } else if x == 0.0 {
                    Ok(Value::Real(f64::NEG_INFINITY))
                } else {
                    // Negative: log(x) = log(|x|) + πi
                    let c = Complex64::new(x, 0.0);
                    let result = c.ln();
                    Ok(complex64_to_value(result))
                }
            }
        }
    }

    /// Logarithm with specified base: log_base(x)
    pub fn numeric_log_base(&self, base: &Value) -> Result<Value, NumericError> {
        let x_log = self.numeric_log()?;
        let base_log = base.numeric_log()?;
        x_log.numeric_div(&base_log)
    }

    /// Square root
    ///
    /// For negative real numbers, returns complex result.
    /// Per R7RS, the principal square root has non-negative imaginary part.
    pub fn numeric_sqrt(&self) -> Result<Value, NumericError> {
        match self {
            Value::Complex(parts) => {
                let (ref r, ref i) = **parts;
                let re_in = r.to_f64()?;
                let im_in = i.to_f64()?;

                // Special case: on negative real axis (real < 0, imag == 0 or -0)
                // R7RS says sqrt returns principal value with non-negative imag
                if re_in < 0.0 && im_in == 0.0 {
                    let abs_sqrt = (-re_in).sqrt();
                    return Ok(Value::Complex(Box::new((
                        Value::Real(0.0),
                        Value::Real(abs_sqrt),
                    ))));
                }

                let c = Complex64::new(re_in, im_in);
                let result = c.sqrt();
                Ok(complex64_to_value(result))
            }
            _ => {
                let x = self.to_f64()?;
                if x >= 0.0 {
                    Ok(Value::Real(x.sqrt()))
                } else {
                    // Negative: sqrt(x) = i * sqrt(|x|)
                    let abs_sqrt = (-x).sqrt();
                    Ok(Value::Complex(Box::new((
                        Value::Real(0.0),
                        Value::Real(abs_sqrt),
                    ))))
                }
            }
        }
    }

    /// Exponentiation: self^exponent
    ///
    /// Special cases:
    /// - 0^0 = 1
    /// - exact^exact_non_negative_integer = exact (when possible)
    pub fn numeric_expt(&self, exponent: &Value) -> Result<Value, NumericError> {
        use super::exactness::{apply_contagion, result_exactness};

        let exactness = result_exactness(self, exponent);

        // Special case: 0^0 = 1 (with inexact contagion)
        if self.is_numeric_zero() && exponent.is_numeric_zero() {
            return Ok(apply_contagion(Value::Integer(1), exactness));
        }

        // Special case: 1^anything = 1 (with inexact contagion)
        if let Value::Integer(1) = self {
            return Ok(apply_contagion(Value::Integer(1), exactness));
        }

        // Check for exact base with exact non-negative integer exponent
        if self.is_exact() && exponent.is_exact() {
            if let Some(exp_int) = exponent_as_integer(exponent) {
                if exp_int >= 0 {
                    return exact_integer_power(self, exp_int as u64);
                } else {
                    // Negative integer exponent: result is 1/base^|exp|
                    let pos_result = exact_integer_power(self, (-exp_int) as u64)?;
                    return Value::Integer(1).numeric_div(&pos_result);
                }
            }
        }

        // General case: use floating point
        match (self, exponent) {
            (Value::Complex(base_parts), Value::Complex(exp_parts)) => {
                let (ref br, ref bi) = **base_parts;
                let (ref er, ref ei) = **exp_parts;
                let base = Complex64::new(br.to_f64()?, bi.to_f64()?);
                let exp = Complex64::new(er.to_f64()?, ei.to_f64()?);
                let result = base.powc(exp);
                Ok(complex64_to_value(result))
            }
            (Value::Complex(base_parts), _) => {
                let (ref br, ref bi) = **base_parts;
                let base = Complex64::new(br.to_f64()?, bi.to_f64()?);
                let exp = exponent.to_f64()?;
                let result = base.powf(exp);
                Ok(complex64_to_value(result))
            }
            (_, Value::Complex(exp_parts)) => {
                let (ref er, ref ei) = **exp_parts;
                let base = self.to_f64()?;
                let exp = Complex64::new(er.to_f64()?, ei.to_f64()?);
                let base_c = Complex64::new(base, 0.0);
                let result = base_c.powc(exp);
                Ok(complex64_to_value(result))
            }
            _ => {
                let base = self.to_f64()?;
                let exp = exponent.to_f64()?;

                // Check if result might be complex (negative base, non-integer exponent)
                if base < 0.0 && exp.fract() != 0.0 {
                    let base_c = Complex64::new(base, 0.0);
                    let result = base_c.powf(exp);
                    Ok(complex64_to_value(result))
                } else {
                    Ok(Value::Real(base.powf(exp)))
                }
            }
        }
    }

    /// Square: x²
    pub fn numeric_square(&self) -> Result<Value, NumericError> {
        self.numeric_mul(self)
    }
}

// =============================================================================
// Helper functions
// =============================================================================

/// Convert Complex64 to Value, simplifying if imaginary is near zero
fn complex64_to_value(c: Complex64) -> Value {
    if c.im.abs() < 1e-15 {
        Value::Real(c.re)
    } else {
        Value::Complex(Box::new((Value::Real(c.re), Value::Real(c.im))))
    }
}

/// Extract integer value from exponent if it's an exact integer
fn exponent_as_integer(v: &Value) -> Option<i64> {
    match v {
        Value::Integer(n) => Some(*n),
        Value::BigInteger(n) => n.to_i64(),
        Value::Rational(r) if r.is_integer() => r.numer().to_i64(),
        _ => None,
    }
}

/// Compute exact integer power: base^exp where exp is non-negative
fn exact_integer_power(base: &Value, exp: u64) -> Result<Value, NumericError> {
    if exp == 0 {
        return Ok(Value::Integer(1));
    }
    if exp == 1 {
        return Ok(base.clone());
    }

    match base {
        Value::Integer(b) => {
            // Try to compute as i64, fall back to BigInt on overflow
            if let Some(result) = checked_pow_i64(*b, exp) {
                Ok(Value::Integer(result))
            } else {
                let big_base = BigInt::from(*b);
                let result = big_base.pow(exp as u32);
                Ok(bigint_to_value(result))
            }
        }
        Value::BigInteger(b) => {
            let result = b.pow(exp as u32);
            Ok(bigint_to_value(result))
        }
        Value::Rational(r) => {
            let numer = r.numer().pow(exp as u32);
            let denom = r.denom().pow(exp as u32);
            Ok(rational_to_value(BigRational::new(numer, denom)))
        }
        _ => {
            // Fall back to floating point
            let f = base.to_f64()?;
            Ok(Value::Real(f.powi(exp as i32)))
        }
    }
}

/// Checked integer power for i64
fn checked_pow_i64(base: i64, exp: u64) -> Option<i64> {
    if exp > 62 {
        return None; // Would definitely overflow
    }

    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp;

    while e > 0 {
        if e & 1 == 1 {
            result = result.checked_mul(b)?;
        }
        e >>= 1;
        if e > 0 {
            b = b.checked_mul(b)?;
        }
    }

    Some(result)
}

/// Convert BigInt to the simplest integer Value
fn bigint_to_value(n: BigInt) -> Value {
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
    fn test_sin() {
        let result = Value::Integer(0).numeric_sin().unwrap();
        assert_real(&result, 0.0);
    }

    #[test]
    fn test_cos() {
        let result = Value::Integer(0).numeric_cos().unwrap();
        assert_real(&result, 1.0);
    }

    #[test]
    fn test_sqrt() {
        let result = Value::Integer(4).numeric_sqrt().unwrap();
        assert_real(&result, 2.0);
    }

    #[test]
    fn test_sqrt_negative() {
        let result = Value::Integer(-1).numeric_sqrt().unwrap();
        // sqrt(-1) = i
        match result {
            Value::Complex(parts) => {
                let (Value::Real(r), Value::Real(i)) = *parts else {
                    panic!("Expected Real parts");
                };
                assert!(r.abs() < 1e-10);
                assert!((i - 1.0).abs() < 1e-10);
            }
            _ => panic!("Expected Complex"),
        }
    }

    #[test]
    fn test_expt_exact() {
        // 2^3 = 8 (exact)
        let result = Value::Integer(2).numeric_expt(&Value::Integer(3)).unwrap();
        assert_integer(&result, 8);

        // 2^-1 = 1/2 (exact)
        let result = Value::Integer(2).numeric_expt(&Value::Integer(-1)).unwrap();
        match result {
            Value::Rational(r) => {
                assert_eq!(r, BigRational::new(1.into(), 2.into()));
            }
            _ => panic!("Expected Rational"),
        }
    }

    #[test]
    fn test_expt_zero() {
        // 0^0 = 1
        let result = Value::Integer(0).numeric_expt(&Value::Integer(0)).unwrap();
        assert_integer(&result, 1);
    }

    #[test]
    fn test_log() {
        let result = Value::Integer(1).numeric_log().unwrap();
        assert_real(&result, 0.0);
    }

    #[test]
    fn test_log_negative() {
        // log(-1) = πi
        let result = Value::Integer(-1).numeric_log().unwrap();
        match result {
            Value::Complex(parts) => {
                let (Value::Real(r), Value::Real(i)) = *parts else {
                    panic!("Expected Real parts");
                };
                assert!(r.abs() < 1e-10);
                assert!((i - std::f64::consts::PI).abs() < 1e-10);
            }
            _ => panic!("Expected Complex"),
        }
    }

    #[test]
    fn test_exp() {
        let result = Value::Integer(0).numeric_exp().unwrap();
        assert_real(&result, 1.0);
    }
}
