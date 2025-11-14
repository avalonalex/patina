//! Arithmetic and numeric primitive operations (R7RS Section 6.2)
//!
//! Implements the numeric tower operations including:
//! - Basic arithmetic (+, -, *, /)
//! - Numeric comparisons (=, <, >, <=, >=)
//! - Integer division (quotient, remainder, modulo)
//! - Numeric utilities (abs, max, min)

use super::super::Evaluator;
use super::super::error::EvalError;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use patina_runtime::value::Value;

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
            (Real(a), BigInteger(b)) | (BigInteger(b), Real(a)) => Real(a * BigInteger(b).to_f64()),
            (Real(a), Rational(r)) | (Rational(r), Real(a)) => Real(a * Rational(r).to_f64()),
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
            )));
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
                )));
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
            )));
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
                )));
            }
        }
    }

    Ok(Value::Integer(min_val))
}

// ========== Rounding Functions ==========

/// (floor x) - Rounds x toward negative infinity
pub(super) fn floor(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "floor")?;

    match &args[0] {
        Value::Integer(n) => Ok(Value::Integer(*n)), // Already exact
        Value::BigInteger(n) => Ok(Value::BigInteger(n.clone())), // Already exact
        Value::Rational(r) => {
            // Floor of rational: largest integer <= r
            let floored = r.floor();
            Ok(Value::BigInteger(floored.numer().clone()))
        }
        Value::Real(f) => Ok(Value::Real(f.floor())),
        other => Err(EvalError::TypeError(format!(
            "floor expects a real number, got {}",
            other.type_name()
        ))),
    }
}

/// (ceiling x) - Rounds x toward positive infinity
pub(super) fn ceiling(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "ceiling")?;

    match &args[0] {
        Value::Integer(n) => Ok(Value::Integer(*n)), // Already exact
        Value::BigInteger(n) => Ok(Value::BigInteger(n.clone())), // Already exact
        Value::Rational(r) => {
            // Ceiling of rational: smallest integer >= r
            let ceiled = r.ceil();
            Ok(Value::BigInteger(ceiled.numer().clone()))
        }
        Value::Real(f) => Ok(Value::Real(f.ceil())),
        other => Err(EvalError::TypeError(format!(
            "ceiling expects a real number, got {}",
            other.type_name()
        ))),
    }
}

/// (truncate x) - Rounds x toward zero
pub(super) fn truncate(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "truncate")?;

    match &args[0] {
        Value::Integer(n) => Ok(Value::Integer(*n)), // Already exact
        Value::BigInteger(n) => Ok(Value::BigInteger(n.clone())), // Already exact
        Value::Rational(r) => {
            // Truncate: round toward zero
            let truncated = r.trunc();
            Ok(Value::BigInteger(truncated.numer().clone()))
        }
        Value::Real(f) => Ok(Value::Real(f.trunc())),
        other => Err(EvalError::TypeError(format!(
            "truncate expects a real number, got {}",
            other.type_name()
        ))),
    }
}

/// (round x) - Rounds x to nearest integer (banker's rounding: ties round to even)
pub(super) fn round(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "round")?;

    match &args[0] {
        Value::Integer(n) => Ok(Value::Integer(*n)), // Already exact
        Value::BigInteger(n) => Ok(Value::BigInteger(n.clone())), // Already exact
        Value::Rational(r) => {
            // R7RS requires banker's rounding (round to even on .5)
            let r_f64 = r.to_f64().unwrap_or(0.0);
            let rounded = if r_f64.fract().abs() == 0.5 {
                // Tie case: round to even
                let floor_val = r_f64.floor();
                if floor_val as i64 % 2 == 0 {
                    floor_val
                } else {
                    r_f64.ceil()
                }
            } else {
                r_f64.round()
            };
            Ok(Value::BigInteger(BigInt::from(rounded as i64)))
        }
        Value::Real(f) => {
            // Banker's rounding for floats too
            let rounded = if f.fract().abs() == 0.5 {
                let floor_val = f.floor();
                if floor_val as i64 % 2 == 0 {
                    floor_val
                } else {
                    f.ceil()
                }
            } else {
                f.round()
            };
            Ok(Value::Real(rounded))
        }
        other => Err(EvalError::TypeError(format!(
            "round expects a real number, got {}",
            other.type_name()
        ))),
    }
}

// ========== Square Root and Exponentiation ==========

/// (sqrt x) - Square root
pub(super) fn sqrt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "sqrt")?;

    let num = NumericValue::from_value(args[0].clone())?;
    let f = num.to_f64();

    if f < 0.0 {
        // TODO: Return complex number for negative sqrt
        return Err(EvalError::InternalError(
            "sqrt of negative number not yet supported (requires complex numbers)".to_string(),
        ));
    }

    Ok(Value::Real(f.sqrt()))
}

/// (square x) - Square of x
pub(super) fn square(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "square")?;

    let num = NumericValue::from_value(args[0].clone())?;
    let result = num.clone().multiply(num);
    Ok(result.into_value(_eval))
}

/// (expt base power) - Exponentiation
pub(super) fn expt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "expt")?;

    let base_num = NumericValue::from_value(args[0].clone())?;
    let power_num = NumericValue::from_value(args[1].clone())?;

    // Handle special case: 0^0 = 1 by convention
    if base_num.is_zero() && power_num.is_zero() {
        return Ok(Value::Integer(1));
    }

    // Simple case: base^inexact or inexact^power -> always inexact
    if !base_num.is_exact() || !power_num.is_exact() {
        let base_f = base_num.to_f64();
        let power_f = power_num.to_f64();
        return Ok(Value::Real(base_f.powf(power_f)));
    }

    // Exact case: try to keep exactness
    match (&base_num, &power_num) {
        (NumericValue::Integer(b), NumericValue::Integer(p)) if *p >= 0 => {
            // Positive integer power - keep exact
            let mut result = BigInt::from(1);
            let base_big = BigInt::from(*b);
            for _ in 0..*p {
                result *= &base_big;
            }

            // Try to fit in i64
            match result.to_i64() {
                Some(n) => Ok(Value::Integer(n)),
                None => Ok(Value::BigInteger(result)),
            }
        }
        (NumericValue::Integer(b), NumericValue::Integer(p)) if *p < 0 => {
            // Negative integer power: base^(-p) = 1/(base^p)
            // Result is rational
            let power_abs = (-*p) as u32;
            let base_big = BigInt::from(*b);
            let numerator = BigInt::from(1);
            let denominator = base_big.pow(power_abs);
            let ratio = BigRational::new(numerator, denominator);

            // Simplify if denominator is 1
            if ratio.denom() == &BigInt::from(1) {
                match ratio.numer().to_i64() {
                    Some(n) => Ok(Value::Integer(n)),
                    None => Ok(Value::BigInteger(ratio.numer().clone())),
                }
            } else {
                Ok(Value::Rational(ratio))
            }
        }
        _ => {
            // Fall back to inexact
            let base_f = base_num.to_f64();
            let power_f = power_num.to_f64();
            Ok(Value::Real(base_f.powf(power_f)))
        }
    }
}

// ========== Float Predicates ==========

/// (finite? x) - Returns #t if x is finite
pub(super) fn finite_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "finite?")?;

    let result = match &args[0] {
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => true,
        Value::Real(f) => f.is_finite(),
        Value::Complex(r, i) => r.is_finite() && i.is_finite(),
        other => {
            return Err(EvalError::TypeError(format!(
                "finite? expects a number, got {}",
                other.type_name()
            )));
        }
    };

    Ok(Value::Boolean(result))
}

/// (infinite? x) - Returns #t if x is infinite
pub(super) fn infinite_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "infinite?")?;

    let result = match &args[0] {
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => false,
        Value::Real(f) => f.is_infinite(),
        Value::Complex(r, i) => r.is_infinite() || i.is_infinite(),
        other => {
            return Err(EvalError::TypeError(format!(
                "infinite? expects a number, got {}",
                other.type_name()
            )));
        }
    };

    Ok(Value::Boolean(result))
}

/// (nan? x) - Returns #t if x is NaN
pub(super) fn nan_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "nan?")?;

    let result = match &args[0] {
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => false,
        Value::Real(f) => f.is_nan(),
        Value::Complex(r, i) => r.is_nan() || i.is_nan(),
        other => {
            return Err(EvalError::TypeError(format!(
                "nan? expects a number, got {}",
                other.type_name()
            )));
        }
    };

    Ok(Value::Boolean(result))
}

// ========== Trigonometric Functions ==========

/// (sin x) - Sine
pub(super) fn sin(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "sin")?;
    let num = NumericValue::from_value(args[0].clone())?;
    Ok(Value::Real(num.to_f64().sin()))
}

/// (cos x) - Cosine
pub(super) fn cos(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "cos")?;
    let num = NumericValue::from_value(args[0].clone())?;
    Ok(Value::Real(num.to_f64().cos()))
}

/// (tan x) - Tangent
pub(super) fn tan(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "tan")?;
    let num = NumericValue::from_value(args[0].clone())?;
    Ok(Value::Real(num.to_f64().tan()))
}

/// (asin x) - Arc sine
pub(super) fn asin(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "asin")?;
    let num = NumericValue::from_value(args[0].clone())?;
    Ok(Value::Real(num.to_f64().asin()))
}

/// (acos x) - Arc cosine
pub(super) fn acos(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "acos")?;
    let num = NumericValue::from_value(args[0].clone())?;
    Ok(Value::Real(num.to_f64().acos()))
}

/// (atan x [y]) - Arc tangent (one or two arguments)
pub(super) fn atan(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_range(&args, 1, 2, "atan")?;

    if args.len() == 1 {
        // One-argument form: atan(x)
        let num = NumericValue::from_value(args[0].clone())?;
        Ok(Value::Real(num.to_f64().atan()))
    } else {
        // Two-argument form: atan2(y, x)
        let y = NumericValue::from_value(args[0].clone())?;
        let x = NumericValue::from_value(args[1].clone())?;
        Ok(Value::Real(y.to_f64().atan2(x.to_f64())))
    }
}

// ========== Exponential and Logarithmic Functions ==========

/// (exp x) - e^x
pub(super) fn exp(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exp")?;
    let num = NumericValue::from_value(args[0].clone())?;
    Ok(Value::Real(num.to_f64().exp()))
}

/// (log x [base]) - Natural log or log with base
pub(super) fn log(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_range(&args, 1, 2, "log")?;

    if args.len() == 1 {
        // One-argument form: natural log
        let num = NumericValue::from_value(args[0].clone())?;
        Ok(Value::Real(num.to_f64().ln()))
    } else {
        // Two-argument form: log with base
        let x = NumericValue::from_value(args[0].clone())?;
        let base = NumericValue::from_value(args[1].clone())?;
        Ok(Value::Real(x.to_f64().log(base.to_f64())))
    }
}

// ========== Number Theory Functions ==========

/// Helper: compute GCD of two BigInts using Euclidean algorithm
fn bigint_gcd(a: &BigInt, b: &BigInt) -> BigInt {
    let mut a = a.abs();
    let mut b = b.abs();

    while !b.is_zero() {
        let temp = b.clone();
        b = &a % &b;
        a = temp;
    }

    a
}

/// (gcd n1 n2 ...) - Greatest common divisor
pub(super) fn gcd(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // R7RS: (gcd) returns 0
    if args.is_empty() {
        return Ok(Value::Integer(0));
    }

    // Convert all arguments to BigInt for uniform handling
    let mut result = BigInt::from(0);

    for arg in args {
        match arg {
            Value::Integer(n) => {
                result = bigint_gcd(&result, &BigInt::from(n));
            }
            Value::BigInteger(n) => {
                result = bigint_gcd(&result, &n);
            }
            other => {
                return Err(EvalError::TypeError(format!(
                    "gcd expects exact integers, got {}",
                    other.type_name()
                )));
            }
        }
    }

    // Try to fit in i64
    match result.to_i64() {
        Some(n) => Ok(Value::Integer(n)),
        None => Ok(Value::BigInteger(result)),
    }
}

/// (lcm n1 n2 ...) - Least common multiple
pub(super) fn lcm(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // R7RS: (lcm) returns 1
    if args.is_empty() {
        return Ok(Value::Integer(1));
    }

    // Convert all arguments to BigInt
    let mut result = BigInt::from(1);

    for arg in args {
        let n = match arg {
            Value::Integer(n) => BigInt::from(n),
            Value::BigInteger(n) => n,
            other => {
                return Err(EvalError::TypeError(format!(
                    "lcm expects exact integers, got {}",
                    other.type_name()
                )));
            }
        };

        // LCM(a, b) = |a * b| / GCD(a, b)
        // But we need to handle zero specially
        if n.is_zero() {
            return Ok(Value::Integer(0));
        }

        let gcd_val = bigint_gcd(&result, &n);
        result = (result * n).abs() / gcd_val;
    }

    // Try to fit in i64
    match result.to_i64() {
        Some(n) => Ok(Value::Integer(n)),
        None => Ok(Value::BigInteger(result)),
    }
}

// ========== Rational Number Accessors ==========

/// (numerator q) - Returns the numerator of rational q
pub(super) fn numerator(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "numerator")?;

    match &args[0] {
        Value::Integer(n) => Ok(Value::Integer(*n)), // Integer's numerator is itself
        Value::BigInteger(n) => Ok(Value::BigInteger(n.clone())),
        Value::Rational(r) => {
            // Return the numerator (already in lowest terms)
            match r.numer().to_i64() {
                Some(n) => Ok(Value::Integer(n)),
                None => Ok(Value::BigInteger(r.numer().clone())),
            }
        }
        other => Err(EvalError::TypeError(format!(
            "numerator expects a rational number, got {}",
            other.type_name()
        ))),
    }
}

/// (denominator q) - Returns the denominator of rational q
pub(super) fn denominator(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "denominator")?;

    match &args[0] {
        Value::Integer(_) | Value::BigInteger(_) => Ok(Value::Integer(1)), // Integer's denominator is 1
        Value::Rational(r) => {
            // Return the denominator (already in lowest terms)
            match r.denom().to_i64() {
                Some(n) => Ok(Value::Integer(n)),
                None => Ok(Value::BigInteger(r.denom().clone())),
            }
        }
        other => Err(EvalError::TypeError(format!(
            "denominator expects a rational number, got {}",
            other.type_name()
        ))),
    }
}

// ========== Exactness Conversion ==========

/// (exact z) - Convert to exact representation (inexact->exact)
pub(super) fn exact(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exact")?;

    match &args[0] {
        // Already exact - return as-is
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => Ok(args[0].clone()),

        // Convert inexact to exact
        Value::Real(f) => {
            // Convert float to rational
            use num_traits::FromPrimitive;

            if f.is_nan() || f.is_infinite() {
                return Err(EvalError::TypeError(
                    "exact: cannot convert NaN or infinity to exact".to_string(),
                ));
            }

            // Convert to rational using BigRational::from_f64
            match BigRational::from_f64(*f) {
                Some(ratio) => {
                    // Simplify to integer if possible
                    if ratio.denom() == &BigInt::from(1) {
                        match ratio.numer().to_i64() {
                            Some(n) => Ok(Value::Integer(n)),
                            None => Ok(Value::BigInteger(ratio.numer().clone())),
                        }
                    } else {
                        Ok(Value::Rational(ratio))
                    }
                }
                None => Err(EvalError::TypeError(
                    "exact: cannot convert float to exact rational".to_string(),
                )),
            }
        }

        other => Err(EvalError::TypeError(format!(
            "exact expects a number, got {}",
            other.type_name()
        ))),
    }
}

/// (inexact z) - Convert to inexact representation (exact->inexact)
pub(super) fn inexact(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "inexact")?;

    match &args[0] {
        // Already inexact - return as-is
        Value::Real(f) => Ok(Value::Real(*f)),

        // Convert exact to inexact
        Value::Integer(n) => Ok(Value::Real(*n as f64)),
        Value::BigInteger(n) => Ok(Value::Real(n.to_f64().unwrap_or(f64::INFINITY))),
        Value::Rational(r) => Ok(Value::Real(r.to_f64().unwrap_or(f64::INFINITY))),

        other => Err(EvalError::TypeError(format!(
            "inexact expects a number, got {}",
            other.type_name()
        ))),
    }
}

// ========== Complex Number Operations ==========

/// (real-part z) - Real part of complex number
pub(super) fn real_part(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "real-part")?;

    match &args[0] {
        Value::Complex(r, _) => Ok(Value::Real(*r)),
        // Real numbers have real part = themselves
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) | Value::Real(_) => {
            Ok(args[0].clone())
        }
        other => Err(EvalError::TypeError(format!(
            "real-part expects a number, got {}",
            other.type_name()
        ))),
    }
}

/// (imag-part z) - Imaginary part of complex number
pub(super) fn imag_part(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "imag-part")?;

    match &args[0] {
        Value::Complex(_, i) => Ok(Value::Real(*i)),
        // Real numbers have imaginary part = 0
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => Ok(Value::Integer(0)),
        Value::Real(_) => Ok(Value::Real(0.0)),
        other => Err(EvalError::TypeError(format!(
            "imag-part expects a number, got {}",
            other.type_name()
        ))),
    }
}

/// (magnitude z) - Magnitude of complex number
pub(super) fn magnitude(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "magnitude")?;

    match &args[0] {
        Value::Complex(r, i) => Ok(Value::Real((r * r + i * i).sqrt())),
        Value::Integer(n) => Ok(Value::Real((*n as f64).abs())),
        Value::BigInteger(n) => Ok(Value::Real(n.to_f64().unwrap_or(f64::INFINITY).abs())),
        Value::Rational(r) => Ok(Value::Real(r.to_f64().unwrap_or(f64::INFINITY).abs())),
        Value::Real(f) => Ok(Value::Real(f.abs())),
        other => Err(EvalError::TypeError(format!(
            "magnitude expects a number, got {}",
            other.type_name()
        ))),
    }
}

/// (angle z) - Angle of complex number in radians
pub(super) fn angle(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "angle")?;

    match &args[0] {
        Value::Complex(r, i) => Ok(Value::Real(i.atan2(*r))),
        Value::Integer(n) if *n >= 0 => Ok(Value::Real(0.0)),
        Value::Integer(_) => Ok(Value::Real(std::f64::consts::PI)), // Negative
        Value::BigInteger(n) if n >= &BigInt::from(0) => Ok(Value::Real(0.0)),
        Value::BigInteger(_) => Ok(Value::Real(std::f64::consts::PI)),
        Value::Rational(r) if r >= &BigRational::from(BigInt::from(0)) => Ok(Value::Real(0.0)),
        Value::Rational(_) => Ok(Value::Real(std::f64::consts::PI)),
        Value::Real(f) if *f >= 0.0 => Ok(Value::Real(0.0)),
        Value::Real(_) => Ok(Value::Real(std::f64::consts::PI)),
        other => Err(EvalError::TypeError(format!(
            "angle expects a number, got {}",
            other.type_name()
        ))),
    }
}

/// (make-rectangular real imag) - Construct complex number from real and imaginary parts
pub(super) fn make_rectangular(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "make-rectangular")?;

    // Convert both parts to f64
    let real = NumericValue::from_value(args[0].clone())?.to_f64();
    let imag = NumericValue::from_value(args[1].clone())?.to_f64();

    // If imaginary part is 0, return just the real part
    if imag == 0.0 {
        // Try to keep exactness if input was exact
        match &args[0] {
            Value::Integer(n) => Ok(Value::Integer(*n)),
            Value::BigInteger(n) => Ok(Value::BigInteger(n.clone())),
            Value::Rational(r) => Ok(Value::Rational(r.clone())),
            _ => Ok(Value::Real(real)),
        }
    } else {
        Ok(Value::Complex(real, imag))
    }
}

/// (make-polar magnitude angle) - Construct complex number from polar coordinates
pub(super) fn make_polar(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "make-polar")?;

    let mag = NumericValue::from_value(args[0].clone())?.to_f64();
    let ang = NumericValue::from_value(args[1].clone())?.to_f64();

    let real = mag * ang.cos();
    let imag = mag * ang.sin();

    // If imaginary part is essentially 0, return just the real part
    if imag.abs() < 1e-10 {
        Ok(Value::Real(real))
    } else {
        Ok(Value::Complex(real, imag))
    }
}

// ========== Additional Number Theory Functions ==========

/// (exact-integer-sqrt k) - Returns two values s and r where k = s^2 + r, and k < (s+1)^2
/// This is the integer square root with remainder
pub(super) fn exact_integer_sqrt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exact-integer-sqrt")?;

    // Convert to BigInt for uniform handling
    let k = match &args[0] {
        Value::Integer(n) if *n < 0 => {
            return Err(EvalError::TypeError(
                "exact-integer-sqrt expects a non-negative integer".to_string(),
            ));
        }
        Value::Integer(n) => BigInt::from(*n),
        Value::BigInteger(n) if n < &BigInt::from(0) => {
            return Err(EvalError::TypeError(
                "exact-integer-sqrt expects a non-negative integer".to_string(),
            ));
        }
        Value::BigInteger(n) => n.clone(),
        other => {
            return Err(EvalError::TypeError(format!(
                "exact-integer-sqrt expects an exact integer, got {}",
                other.type_name()
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

        // s is the integer square root
        // r = k - s^2 is the remainder
        let r = &k - &s * &s;
        (s, r)
    };

    // Convert s and r to appropriate Value types
    let s_val = match s.to_i64() {
        Some(n) => Value::Integer(n),
        None => Value::BigInteger(s),
    };

    let r_val = match r.to_i64() {
        Some(n) => Value::Integer(n),
        None => Value::BigInteger(r),
    };

    // Return as multiple values using Value::Values
    Ok(Value::Values(vec![s_val, r_val]))
}

/// (rationalize x tolerance) - Find simplest rational within tolerance of x
/// Returns the rational with smallest denominator within x±tolerance
pub(super) fn rationalize(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "rationalize")?;

    let x = NumericValue::from_value(args[0].clone())?;
    let tolerance = NumericValue::from_value(args[1].clone())?;

    let x_f64 = x.to_f64();
    let tol_f64 = tolerance.to_f64().abs();

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

    let result = simplest_rational_in_range(lower, upper);

    // Convert to appropriate Value
    Ok(_eval.rational_to_value(result))
}

/// Register arithmetic primitives with the registry
///
/// Registers (scheme base) arithmetic primitives with their full namespace.
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Addition
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "+",
        Arity::Min(0),
        "Returns the sum of its arguments. With no arguments, returns 0.",
        |eval, args, _tail| add(eval, args).map(EvalResult::Value),
    ));

    // Subtraction
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "-",
        Arity::Min(1),
        "Subtracts subsequent arguments from the first. With one argument, returns its negation.",
        |eval, args, _tail| subtract(eval, args).map(EvalResult::Value),
    ));

    // Multiplication
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "*",
        Arity::Min(0),
        "Returns the product of its arguments. With no arguments, returns 1.",
        |eval, args, _tail| multiply(eval, args).map(EvalResult::Value),
    ));

    // Floor
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "floor",
        Arity::Exact(1),
        "Returns the largest integer not larger than x.",
        |eval, args, _tail| floor(eval, args).map(EvalResult::Value),
    ));

    // Division
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "/",
        Arity::Min(1),
        "Divides the first argument by subsequent arguments. With one argument, returns its reciprocal.",
        |eval, args, _tail| divide(eval, args).map(EvalResult::Value),
    ));

    // Numeric equality
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "=",
        Arity::Min(2),
        "Returns #t if all arguments are numerically equal.",
        |eval, args, _tail| numeric_equal(eval, args).map(EvalResult::Value),
    ));

    // Less than
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "<",
        Arity::Min(2),
        "Returns #t if arguments are monotonically increasing.",
        |eval, args, _tail| less_than(eval, args).map(EvalResult::Value),
    ));

    // Greater than
    registry.register(PrimitiveFn::new(
        "scheme.base",
        ">",
        Arity::Min(2),
        "Returns #t if arguments are monotonically decreasing.",
        |eval, args, _tail| greater_than(eval, args).map(EvalResult::Value),
    ));

    // Less than or equal
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "<=",
        Arity::Min(2),
        "Returns #t if arguments are monotonically non-decreasing.",
        |eval, args, _tail| less_equal(eval, args).map(EvalResult::Value),
    ));

    // Greater than or equal
    registry.register(PrimitiveFn::new(
        "scheme.base",
        ">=",
        Arity::Min(2),
        "Returns #t if arguments are monotonically non-increasing.",
        |eval, args, _tail| greater_equal(eval, args).map(EvalResult::Value),
    ));

    // Quotient
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "quotient",
        Arity::Exact(2),
        "Returns the quotient of dividing n1 by n2.",
        |eval, args, _tail| quotient(eval, args).map(EvalResult::Value),
    ));

    // Remainder
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "remainder",
        Arity::Exact(2),
        "Returns the remainder of dividing n1 by n2.",
        |eval, args, _tail| remainder(eval, args).map(EvalResult::Value),
    ));

    // Modulo
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "modulo",
        Arity::Exact(2),
        "Returns n1 modulo n2.",
        |eval, args, _tail| modulo(eval, args).map(EvalResult::Value),
    ));

    // Absolute value
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "abs",
        Arity::Exact(1),
        "Returns the absolute value of x.",
        |eval, args, _tail| abs(eval, args).map(EvalResult::Value),
    ));

    // Maximum
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "max",
        Arity::Min(1),
        "Returns the maximum of its arguments.",
        |eval, args, _tail| max(eval, args).map(EvalResult::Value),
    ));

    // Minimum
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "min",
        Arity::Min(1),
        "Returns the minimum of its arguments.",
        |eval, args, _tail| min(eval, args).map(EvalResult::Value),
    ));

    // Ceiling
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "ceiling",
        Arity::Exact(1),
        "Returns the smallest integer not smaller than x.",
        |eval, args, _tail| ceiling(eval, args).map(EvalResult::Value),
    ));

    // Truncate
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "truncate",
        Arity::Exact(1),
        "Returns the integer closest to x whose absolute value is not larger than the absolute value of x.",
        |eval, args, _tail| truncate(eval, args).map(EvalResult::Value),
    ));

    // Round
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "round",
        Arity::Exact(1),
        "Returns the closest integer to x, rounding to even when x is halfway between two integers.",
        |eval, args, _tail| round(eval, args).map(EvalResult::Value),
    ));

    // Square root
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "sqrt",
        Arity::Exact(1),
        "Returns the principal square root of x.",
        |eval, args, _tail| sqrt(eval, args).map(EvalResult::Value),
    ));

    // Square
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "square",
        Arity::Exact(1),
        "Returns the square of x.",
        |eval, args, _tail| square(eval, args).map(EvalResult::Value),
    ));

    // Exponentiation
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "expt",
        Arity::Exact(2),
        "Returns z1 raised to the power z2.",
        |eval, args, _tail| expt(eval, args).map(EvalResult::Value),
    ));

    // Finite predicate
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "finite?",
        Arity::Exact(1),
        "Returns #t if x is finite.",
        |eval, args, _tail| finite_p(eval, args).map(EvalResult::Value),
    ));

    // Infinite predicate
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "infinite?",
        Arity::Exact(1),
        "Returns #t if x is infinite.",
        |eval, args, _tail| infinite_p(eval, args).map(EvalResult::Value),
    ));

    // NaN predicate
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "nan?",
        Arity::Exact(1),
        "Returns #t if x is NaN.",
        |eval, args, _tail| nan_p(eval, args).map(EvalResult::Value),
    ));

    // Sine
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "sin",
        Arity::Exact(1),
        "Returns the sine of x.",
        |eval, args, _tail| sin(eval, args).map(EvalResult::Value),
    ));

    // Cosine
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "cos",
        Arity::Exact(1),
        "Returns the cosine of x.",
        |eval, args, _tail| cos(eval, args).map(EvalResult::Value),
    ));

    // Tangent
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "tan",
        Arity::Exact(1),
        "Returns the tangent of x.",
        |eval, args, _tail| tan(eval, args).map(EvalResult::Value),
    ));

    // Arcsine
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "asin",
        Arity::Exact(1),
        "Returns the arcsine of x.",
        |eval, args, _tail| asin(eval, args).map(EvalResult::Value),
    ));

    // Arccosine
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "acos",
        Arity::Exact(1),
        "Returns the arccosine of x.",
        |eval, args, _tail| acos(eval, args).map(EvalResult::Value),
    ));

    // Arctangent
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "atan",
        Arity::Range(1, 2),
        "Returns the arctangent of x, or of y/x.",
        |eval, args, _tail| atan(eval, args).map(EvalResult::Value),
    ));

    // Exponential
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "exp",
        Arity::Exact(1),
        "Returns e raised to the power x.",
        |eval, args, _tail| exp(eval, args).map(EvalResult::Value),
    ));

    // Natural logarithm
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "log",
        Arity::Range(1, 2),
        "Returns the natural logarithm of x, or logarithm of x in base y.",
        |eval, args, _tail| log(eval, args).map(EvalResult::Value),
    ));

    // Greatest common divisor
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "gcd",
        Arity::Min(0),
        "Returns the greatest common divisor of its arguments.",
        |eval, args, _tail| gcd(eval, args).map(EvalResult::Value),
    ));

    // Least common multiple
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "lcm",
        Arity::Min(0),
        "Returns the least common multiple of its arguments.",
        |eval, args, _tail| lcm(eval, args).map(EvalResult::Value),
    ));

    // Numerator
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "numerator",
        Arity::Exact(1),
        "Returns the numerator of x.",
        |eval, args, _tail| numerator(eval, args).map(EvalResult::Value),
    ));

    // Denominator
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "denominator",
        Arity::Exact(1),
        "Returns the denominator of x.",
        |eval, args, _tail| denominator(eval, args).map(EvalResult::Value),
    ));

    // Exact
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "exact",
        Arity::Exact(1),
        "Returns an exact representation of x.",
        |eval, args, _tail| exact(eval, args).map(EvalResult::Value),
    ));

    // Inexact
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "inexact",
        Arity::Exact(1),
        "Returns an inexact representation of x.",
        |eval, args, _tail| inexact(eval, args).map(EvalResult::Value),
    ));

    // Real part
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "real-part",
        Arity::Exact(1),
        "Returns the real part of z.",
        |eval, args, _tail| real_part(eval, args).map(EvalResult::Value),
    ));

    // Imaginary part
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "imag-part",
        Arity::Exact(1),
        "Returns the imaginary part of z.",
        |eval, args, _tail| imag_part(eval, args).map(EvalResult::Value),
    ));

    // Magnitude
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "magnitude",
        Arity::Exact(1),
        "Returns the magnitude of z.",
        |eval, args, _tail| magnitude(eval, args).map(EvalResult::Value),
    ));

    // Angle
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "angle",
        Arity::Exact(1),
        "Returns the angle of z.",
        |eval, args, _tail| angle(eval, args).map(EvalResult::Value),
    ));

    // Make rectangular
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "make-rectangular",
        Arity::Exact(2),
        "Returns a complex number with real part x1 and imaginary part x2.",
        |eval, args, _tail| make_rectangular(eval, args).map(EvalResult::Value),
    ));

    // Make polar
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "make-polar",
        Arity::Exact(2),
        "Returns a complex number with magnitude x1 and angle x2.",
        |eval, args, _tail| make_polar(eval, args).map(EvalResult::Value),
    ));

    // Exact integer square root
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "exact-integer-sqrt",
        Arity::Exact(1),
        "Returns two values s and r where k = s^2 + r and k < (s+1)^2.",
        |eval, args, _tail| exact_integer_sqrt(eval, args).map(EvalResult::Value),
    ));

    // Rationalize
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "rationalize",
        Arity::Exact(2),
        "Returns the simplest rational number differing from x by no more than y.",
        |eval, args, _tail| rationalize(eval, args).map(EvalResult::Value),
    ));
}
