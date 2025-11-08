//! Arithmetic and numeric primitive operations (R7RS Section 6.2)
//!
//! Implements the numeric tower operations including:
//! - Basic arithmetic (+, -, *, /)
//! - Numeric comparisons (=, <, >, <=, >=)
//! - Integer division (quotient, remainder, modulo)
//! - Numeric utilities (abs, max, min)

use super::super::error::EvalError;
use super::super::Evaluator;
use crate::value::Value;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};

/// Internal representation for numeric operations
/// Automatically promotes to the most general type needed
#[derive(Debug, Clone)]
enum NumericValue {
    Integer(i64),
    BigInteger(BigInt),
    Rational(BigRational),
    Real(f64),
    Complex(Box<(NumericValue, NumericValue)>), // (real, imaginary) - components must not be Complex
}

impl NumericValue {
    /// Convert an integer to BigInt (helper for type promotion)
    #[inline]
    fn to_bigint(n: i64) -> BigInt {
        BigInt::from(n)
    }

    /// Convert to f64 with overflow handling (helper for inexact contagion)
    #[inline]
    fn to_f64(&self) -> f64 {
        match self {
            NumericValue::Integer(n) => *n as f64,
            NumericValue::BigInteger(n) => n.to_f64().unwrap_or(f64::INFINITY),
            NumericValue::Rational(r) => r.to_f64().unwrap_or(f64::INFINITY),
            NumericValue::Real(f) => *f,
            NumericValue::Complex(_) => panic!("Cannot convert complex to f64"),
        }
    }

    /// Convert to BigRational (helper for exact arithmetic)
    #[allow(dead_code)]
    #[inline]
    fn to_rational(&self) -> BigRational {
        match self {
            NumericValue::Integer(n) => BigRational::from(Self::to_bigint(*n)),
            NumericValue::BigInteger(n) => BigRational::from(n.clone()),
            NumericValue::Rational(r) => r.clone(),
            _ => panic!("Cannot convert inexact number to rational"),
        }
    }

    fn from_value(v: Value) -> Result<Self, EvalError> {
        match v {
            Value::Integer(n) => Ok(NumericValue::Integer(n)),
            Value::BigInteger(n) => Ok(NumericValue::BigInteger(n)),
            Value::Rational(r) => Ok(NumericValue::Rational(r)),
            Value::Real(f) => Ok(NumericValue::Real(f)),
            Value::Complex(real, imag) => {
                // Convert old (f64, f64) to new representation
                Ok(NumericValue::Complex(Box::new((
                    NumericValue::Real(real),
                    NumericValue::Real(imag),
                ))))
            }
            other => Err(EvalError::TypeError(format!(
                "Expected number, got {}",
                other.type_name()
            ))),
        }
    }

    /// Check if a numeric value is zero
    #[allow(dead_code)]
    fn is_zero(&self) -> bool {
        match self {
            NumericValue::Integer(n) => *n == 0,
            NumericValue::BigInteger(n) => n.is_zero(),
            NumericValue::Rational(r) => r.is_zero(),
            NumericValue::Real(f) => *f == 0.0,
            NumericValue::Complex(parts) => {
                let (r, i) = &**parts;
                r.is_zero() && i.is_zero()
            }
        }
    }

    /// Check if a numeric value is exact (not Real or Complex with Real components)
    #[allow(dead_code)]
    fn is_exact(&self) -> bool {
        match self {
            NumericValue::Integer(_) | NumericValue::BigInteger(_) | NumericValue::Rational(_) => {
                true
            }
            NumericValue::Real(_) => false,
            NumericValue::Complex(parts) => {
                let (r, i) = &**parts;
                r.is_exact() && i.is_exact()
            }
        }
    }

    /// Construct a complex number from real and imaginary parts
    /// Returns error if either component is itself complex (R7RS requires real components)
    #[allow(dead_code)]
    fn make_complex(real: NumericValue, imag: NumericValue) -> Result<NumericValue, EvalError> {
        // Ban nested complex numbers (not mathematically meaningful)
        if matches!(real, NumericValue::Complex(_)) {
            return Err(EvalError::TypeError(
                "make-rectangular: real part must be a real number, not complex".to_string(),
            ));
        }
        if matches!(imag, NumericValue::Complex(_)) {
            return Err(EvalError::TypeError(
                "make-rectangular: imaginary part must be a real number, not complex".to_string(),
            ));
        }

        // Optimization: if imaginary is zero, return just real part
        if imag.is_zero() {
            return Ok(real);
        }

        Ok(NumericValue::Complex(Box::new((real, imag))))
    }

    fn into_value(self, evaluator: &Evaluator) -> Value {
        match self {
            NumericValue::Integer(n) => Value::Integer(n),
            NumericValue::BigInteger(n) => Value::BigInteger(n),
            NumericValue::Rational(r) => evaluator.rational_to_value(r),
            NumericValue::Real(f) => Value::Real(f),
            NumericValue::Complex(parts) => {
                let (r, i) = *parts;
                // Convert components to f64 using helper method
                // TODO: Update Value::Complex to use NumericValue
                let real_f64 = r.to_f64();
                let imag_f64 = i.to_f64();
                Value::Complex(real_f64, imag_f64)
            }
        }
    }

    /// Negate a numeric value
    fn negate(self) -> Self {
        use NumericValue::*;
        match self {
            Integer(n) => match n.checked_neg() {
                Some(neg) => Integer(neg),
                None => BigInteger(-Self::to_bigint(n)),
            },
            BigInteger(n) => BigInteger(-n),
            Rational(r) => Rational(-r),
            Real(f) => Real(-f),
            Complex(parts) => {
                let (r, i) = *parts;
                Complex(Box::new((r.negate(), i.negate())))
            }
        }
    }

    /// Add two numeric values, promoting types as needed
    fn add(self, other: Self) -> Self {
        use NumericValue::*;
        match (self, other) {
            // Integer + Integer (with overflow check)
            (Integer(a), Integer(b)) => match a.checked_add(b) {
                Some(sum) => Integer(sum),
                None => BigInteger(Self::to_bigint(a) + Self::to_bigint(b)),
            },
            // Promote to BigInteger
            (BigInteger(a), Integer(b)) | (Integer(b), BigInteger(a)) => {
                BigInteger(a + Self::to_bigint(b))
            }
            (BigInteger(a), BigInteger(b)) => BigInteger(a + b),
            // Promote to Rational
            (Rational(a), Rational(b)) => Rational(a + b),
            (Rational(a), Integer(b)) | (Integer(b), Rational(a)) => {
                Rational(a + BigRational::from(Self::to_bigint(b)))
            }
            (Rational(a), BigInteger(b)) | (BigInteger(b), Rational(a)) => {
                Rational(a + BigRational::from(b))
            }
            // Promote to Real (inexact contagion)
            (Real(a), Real(b)) => Real(a + b),
            (Real(a), Integer(b)) | (Integer(b), Real(a)) => Real(a + b as f64),
            (Real(a), BigInteger(b)) | (BigInteger(b), Real(a)) => {
                Real(a + b.to_f64().unwrap_or(f64::INFINITY))
            }
            (Real(a), Rational(r)) | (Rational(r), Real(a)) => {
                Real(a + r.to_f64().unwrap_or(f64::INFINITY))
            }
            // Complex number addition: (a+bi) + (c+di) = (a+c) + (b+d)i
            (Complex(parts1), Complex(parts2)) => {
                let (r1, i1) = *parts1;
                let (r2, i2) = *parts2;
                Complex(Box::new((r1.add(r2), i1.add(i2))))
            }
            // Promote real to complex (a + (c+di) = (a+c) + di)
            (Complex(parts), other) | (other, Complex(parts)) => {
                let (r, i) = *parts;
                Complex(Box::new((r.add(other), i)))
            }
        }
    }

    /// Subtract two numeric values
    fn subtract(self, other: Self) -> Self {
        use NumericValue::*;
        match (self, other) {
            (Integer(a), Integer(b)) => match a.checked_sub(b) {
                Some(diff) => Integer(diff),
                None => BigInteger(Self::to_bigint(a) - Self::to_bigint(b)),
            },
            (BigInteger(a), Integer(b)) => BigInteger(a - Self::to_bigint(b)),
            (Integer(a), BigInteger(b)) => BigInteger(Self::to_bigint(a) - b),
            (BigInteger(a), BigInteger(b)) => BigInteger(a - b),
            (Rational(a), Rational(b)) => Rational(a - b),
            (Rational(a), Integer(b)) => Rational(a - BigRational::from(Self::to_bigint(b))),
            (Integer(a), Rational(b)) => Rational(BigRational::from(Self::to_bigint(a)) - b),
            (Rational(a), BigInteger(b)) => Rational(a - BigRational::from(b)),
            (BigInteger(a), Rational(b)) => Rational(BigRational::from(a) - b),
            (Real(a), Real(b)) => Real(a - b),
            (Real(a), Integer(b)) => Real(a - b as f64),
            (Integer(a), Real(b)) => Real(a as f64 - b),
            (Real(a), BigInteger(b)) => Real(a - b.to_f64().unwrap_or(f64::INFINITY)),
            (BigInteger(a), Real(b)) => Real(a.to_f64().unwrap_or(f64::INFINITY) - b),
            (Real(a), Rational(r)) => Real(a - r.to_f64().unwrap_or(f64::INFINITY)),
            (Rational(r), Real(a)) => Real(r.to_f64().unwrap_or(f64::INFINITY) - a),
            // Complex subtraction: (a+bi) - (c+di) = (a-c) + (b-d)i
            (Complex(parts1), Complex(parts2)) => {
                let (r1, i1) = *parts1;
                let (r2, i2) = *parts2;
                Complex(Box::new((r1.subtract(r2), i1.subtract(i2))))
            }
            // Real - Complex
            (other, Complex(parts)) => {
                let (r, i) = *parts;
                Complex(Box::new((other.subtract(r), i.negate())))
            }
            // Complex - Real
            (Complex(parts), other) => {
                let (r, i) = *parts;
                Complex(Box::new((r.subtract(other), i)))
            }
        }
    }

    /// Multiply two numeric values
    fn multiply(self, other: Self) -> Self {
        use NumericValue::*;
        match (self, other) {
            (Integer(a), Integer(b)) => match a.checked_mul(b) {
                Some(product) => Integer(product),
                None => BigInteger(Self::to_bigint(a) * Self::to_bigint(b)),
            },
            (BigInteger(a), Integer(b)) | (Integer(b), BigInteger(a)) => {
                BigInteger(a * Self::to_bigint(b))
            }
            (BigInteger(a), BigInteger(b)) => BigInteger(a * b),
            (Rational(a), Rational(b)) => Rational(a * b),
            (Rational(a), Integer(b)) | (Integer(b), Rational(a)) => {
                Rational(a * BigRational::from(Self::to_bigint(b)))
            }
            (Rational(a), BigInteger(b)) | (BigInteger(b), Rational(a)) => {
                Rational(a * BigRational::from(b))
            }
            (Real(a), Real(b)) => Real(a * b),
            (Real(a), Integer(b)) | (Integer(b), Real(a)) => Real(a * b as f64),
            (Real(a), BigInteger(b)) | (BigInteger(b), Real(a)) => {
                Real(a * BigInteger(b).to_f64())
            }
            (Real(a), Rational(r)) | (Rational(r), Real(a)) => {
                Real(a * Rational(r).to_f64())
            }
            // Complex multiplication: (a+bi)(c+di) = (ac-bd) + (ad+bc)i
            (Complex(parts1), Complex(parts2)) => {
                let (r1, i1) = *parts1;
                let (r2, i2) = *parts2;
                // (a+bi)(c+di) = ac + adi + bci + bdi²
                //               = ac - bd + (ad + bc)i
                let ac = r1.clone().multiply(r2.clone());
                let bd = i1.clone().multiply(i2.clone());
                let ad = r1.multiply(i2.clone());
                let bc = i1.multiply(r2);
                Complex(Box::new((ac.subtract(bd), ad.add(bc))))
            }
            // Promote real to complex
            (Complex(parts), other) | (other, Complex(parts)) => {
                let (r, i) = *parts;
                Complex(Box::new((r.multiply(other.clone()), i.multiply(other))))
            }
        }
    }
}

impl Evaluator {
    /// Helper to convert BigRational to the appropriate Value type
    /// Simplifies to Integer or BigInteger if denominator is 1
    pub(in crate::eval) fn rational_to_value(&self, r: BigRational) -> Value {
        if r.denom() == &BigInt::from(1) {
            // Denominator is 1, simplify to integer
            let numer = r.numer();
            if let Some(n) = numer.to_i64() {
                Value::Integer(n)
            } else {
                Value::BigInteger(numer.clone())
            }
        } else {
            Value::Rational(r)
        }
    }

    /// Helper for exact numeric comparison
    /// Returns (rational_value, is_exact) for proper comparison
    fn value_to_comparable(&self, v: &Value) -> Result<(BigRational, bool), EvalError> {
        match v {
            Value::Integer(n) => Ok((BigRational::from(BigInt::from(*n)), true)),
            Value::BigInteger(n) => Ok((BigRational::from(n.clone()), true)),
            Value::Rational(r) => Ok((r.clone(), true)),
            Value::Real(f) => {
                // For inexact numbers, convert to rational approximation
                // This maintains correct comparison semantics
                use num_rational::Ratio;
                let ratio = Ratio::from_float(*f).ok_or_else(|| {
                    EvalError::TypeError("Cannot compare with NaN or infinity".to_string())
                })?;
                Ok((ratio, false))
            }
            other => Err(EvalError::TypeError(format!(
                "Expected number, got {}",
                other.type_name()
            ))),
        }
    }

    /// Generic numeric comparison helper
    fn primitive_numeric_compare<F>(
        &self,
        args: Vec<Value>,
        op: F,
        op_name: &str,
    ) -> Result<Value, EvalError>
    where
        F: Fn(&BigRational, &BigRational) -> bool,
    {
        self.check_arity_min(&args, 2, op_name)?;

        for i in 0..args.len() - 1 {
            let (a, _) = self.value_to_comparable(&args[i])?;
            let (b, _) = self.value_to_comparable(&args[i + 1])?;
            if !op(&a, &b) {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }
}

// ===== Public Primitive Functions =====

pub(super) fn add(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    let mut result = NumericValue::Integer(0);

    for arg in args {
        let num = NumericValue::from_value(arg)
            .map_err(|_| EvalError::TypeError("+ expects numbers".to_string()))?;
        result = result.add(num);
    }

    Ok(result.into_value(evaluator))
}

pub(super) fn subtract(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    if args.len() == 1 {
        let num = NumericValue::from_value(args[0].clone())
            .map_err(|_| EvalError::TypeError("- expects numbers".to_string()))?;
        return Ok(num.negate().into_value(evaluator));
    }

    let mut result = NumericValue::from_value(args[0].clone())
        .map_err(|_| EvalError::TypeError("- expects numbers".to_string()))?;

    for arg in &args[1..] {
        let num = NumericValue::from_value(arg.clone())
            .map_err(|_| EvalError::TypeError("- expects numbers".to_string()))?;
        result = result.subtract(num);
    }

    Ok(result.into_value(evaluator))
}

pub(super) fn multiply(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    let mut result = NumericValue::Integer(1);

    for arg in args {
        let num = NumericValue::from_value(arg)
            .map_err(|_| EvalError::TypeError("* expects numbers".to_string()))?;
        result = result.multiply(num);
    }

    Ok(result.into_value(evaluator))
}

pub(super) fn divide(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    // Check if any argument is inexact (Real) - if so, use float arithmetic
    let has_inexact = args.iter().any(|v| matches!(v, Value::Real(_)));

    if has_inexact {
        // Inexact contagion: use float arithmetic
        // Division by inexact zero produces +inf.0 or -inf.0 (not an error)
        let to_f64 = |v: &Value| -> Result<f64, EvalError> {
            match v {
                Value::Integer(n) => Ok(*n as f64),
                Value::BigInteger(n) => Ok(n.to_f64().unwrap_or(f64::INFINITY)),
                Value::Rational(r) => Ok(r.to_f64().unwrap_or(f64::INFINITY)),
                Value::Real(n) => Ok(*n),
                other => Err(EvalError::TypeError(format!(
                    "/ expects numbers, got {}",
                    other.type_name()
                ))),
            }
        };

        if args.len() == 1 {
            let x = to_f64(&args[0])?;
            // Inexact division by zero is allowed, produces inf
            return Ok(Value::Real(1.0 / x));
        }

        let mut result = to_f64(&args[0])?;
        for arg in &args[1..] {
            let divisor = to_f64(arg)?;
            // Inexact division by zero is allowed, produces inf
            result /= divisor;
        }
        Ok(Value::Real(result))
    } else {
        // All exact: use rational arithmetic
        let to_rational = |v: &Value| -> Result<BigRational, EvalError> {
            match v {
                Value::Integer(n) => Ok(BigRational::from(BigInt::from(*n))),
                Value::BigInteger(n) => Ok(BigRational::from(n.clone())),
                Value::Rational(r) => Ok(r.clone()),
                other => Err(EvalError::TypeError(format!(
                    "/ expects numbers, got {}",
                    other.type_name()
                ))),
            }
        };

        if args.len() == 1 {
            let x = to_rational(&args[0])?;
            if x.is_zero() {
                return Err(EvalError::DivisionByZero);
            }
            let result = BigRational::from(BigInt::from(1)) / x;
            return Ok(evaluator.rational_to_value(result));
        }

        let mut result = to_rational(&args[0])?;
        for arg in &args[1..] {
            let divisor = to_rational(arg)?;
            if divisor.is_zero() {
                return Err(EvalError::DivisionByZero);
            }
            result /= divisor;
        }
        Ok(evaluator.rational_to_value(result))
    }
}

pub(super) fn numeric_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.primitive_numeric_compare(args, |a, b| a == b, "=")
}

pub(super) fn less_than(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.primitive_numeric_compare(args, |a, b| a < b, "<")
}

pub(super) fn greater_than(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.primitive_numeric_compare(args, |a, b| a > b, ">")
}

pub(super) fn less_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.primitive_numeric_compare(args, |a, b| a <= b, "<=")
}

pub(super) fn greater_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.primitive_numeric_compare(args, |a, b| a >= b, ">=")
}

// ========== Binary Integer Operation Helper ==========

/// Generic helper for binary integer operations with optional division-by-zero checking
///
/// Handles all 4 type combinations (Int×Int, Big×Big, Int×Big, Big×Int) with automatic
/// type promotion and zero checking for division operations.
fn binary_int_op<FInt, FBig>(
    a: &Value,
    b: &Value,
    op_name: &str,
    int_op: FInt,
    big_op: FBig,
    check_zero: bool,
) -> Result<Value, EvalError>
where
    FInt: Fn(i64, i64) -> i64,
    FBig: Fn(&BigInt, &BigInt) -> BigInt,
{
    // Division by zero check if requested
    if check_zero {
        match b {
            Value::Integer(n) if *n == 0 => return Err(EvalError::DivisionByZero),
            Value::BigInteger(n) if n.is_zero() => return Err(EvalError::DivisionByZero),
            _ => {}
        }
    }

    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(int_op(*a, *b))),
        (Value::BigInteger(a), Value::BigInteger(b)) => Ok(Value::BigInteger(big_op(a, b))),
        (Value::Integer(a), Value::BigInteger(b)) => {
            Ok(Value::BigInteger(big_op(&NumericValue::to_bigint(*a), b)))
        }
        (Value::BigInteger(a), Value::Integer(b)) => {
            Ok(Value::BigInteger(big_op(a, &NumericValue::to_bigint(*b))))
        }
        _ => Err(EvalError::TypeError(format!(
            "{} requires integers, got {} and {}",
            op_name,
            a.type_name(),
            b.type_name()
        ))),
    }
}

pub(super) fn quotient(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "quotient")?;
    binary_int_op(
        &args[0],
        &args[1],
        "quotient",
        |a, b| a / b,
        |a, b| a / b,
        true, // check for division by zero
    )
}

pub(super) fn remainder(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "remainder")?;
    binary_int_op(
        &args[0],
        &args[1],
        "remainder",
        |a, b| a % b,
        |a, b| a % b,
        true, // check for division by zero
    )
}

pub(super) fn modulo(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    use num_traits::Euclid;
    evaluator.check_arity_exact(&args, 2, "modulo")?;
    binary_int_op(
        &args[0],
        &args[1],
        "modulo",
        |a, b| a.rem_euclid(b),
        |a, b| a.rem_euclid(b),
        true, // check for division by zero
    )
}

pub(super) fn abs(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "abs")?;

    match &args[0] {
        Value::Integer(n) => {
            if *n == i64::MIN {
                Ok(Value::BigInteger(BigInt::from(*n).abs()))
            } else {
                Ok(Value::Integer(n.abs()))
            }
        }
        Value::BigInteger(n) => Ok(Value::BigInteger(n.abs())),
        Value::Rational(r) => Ok(Value::Rational(r.abs())),
        Value::Real(r) => Ok(Value::Real(r.abs())),
        _ => Err(EvalError::TypeError(format!(
            "abs requires a number, got {}",
            args[0].type_name()
        ))),
    }
}

pub(super) fn max(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "max")?;

    let mut max_val = match &args[0] {
        Value::Integer(n) => *n,
        _ => {
            return Err(EvalError::TypeError(format!(
                "max requires integers, got {}",
                args[0].type_name()
            )))
        }
    };

    for arg in &args[1..] {
        match arg {
            Value::Integer(n) => {
                if *n > max_val {
                    max_val = *n;
                }
            }
            _ => {
                return Err(EvalError::TypeError(format!(
                    "max requires integers, got {}",
                    arg.type_name()
                )))
            }
        }
    }

    Ok(Value::Integer(max_val))
}

pub(super) fn min(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "min")?;

    let mut min_val = match &args[0] {
        Value::Integer(n) => *n,
        _ => {
            return Err(EvalError::TypeError(format!(
                "min requires integers, got {}",
                args[0].type_name()
            )))
        }
    };

    for arg in &args[1..] {
        match arg {
            Value::Integer(n) => {
                if *n < min_val {
                    min_val = *n;
                }
            }
            _ => {
                return Err(EvalError::TypeError(format!(
                    "min requires integers, got {}",
                    arg.type_name()
                )))
            }
        }
    }

    Ok(Value::Integer(min_val))
}
