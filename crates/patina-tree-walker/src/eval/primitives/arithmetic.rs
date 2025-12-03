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
use num_complex::Complex64;
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

    /// Check if a numeric value is NaN
    fn is_nan(&self) -> bool {
        match self {
            NumericValue::Real(f) => f.is_nan(),
            NumericValue::Complex(parts) => {
                let (r, i) = &**parts;
                r.is_nan() || i.is_nan()
            }
            _ => false,
        }
    }

    /// Check if a numeric value is inexact (Real or Complex)
    fn is_inexact(&self) -> bool {
        matches!(self, NumericValue::Real(_) | NumericValue::Complex(_))
    }

    /// Convert a numeric value to inexact (Real)
    fn to_inexact(&self) -> NumericValue {
        match self {
            NumericValue::Real(_) | NumericValue::Complex(_) => self.clone(), // Already inexact
            _ => NumericValue::Real(self.to_f64()),
        }
    }

    /// Compare if this number is greater than another
    /// Returns false if either is NaN or if they can't be compared
    fn greater_than(&self, other: &NumericValue) -> bool {
        // Can't compare complex numbers or NaN
        if self.is_nan() || other.is_nan() {
            return false;
        }
        if matches!(self, NumericValue::Complex(_)) || matches!(other, NumericValue::Complex(_)) {
            return false;
        }

        // Convert both to comparable form (BigRational or f64)
        match (self, other) {
            // If either is Real, use float comparison
            (NumericValue::Real(_), _) | (_, NumericValue::Real(_)) => {
                let a = self.to_f64();
                let b = other.to_f64();
                a > b
            }
            // Both exact - use rational comparison
            _ => {
                let a = self.to_rational();
                let b = other.to_rational();
                a > b
            }
        }
    }

    /// Compare if this number is less than another
    /// Returns false if either is NaN or if they can't be compared
    fn less_than(&self, other: &NumericValue) -> bool {
        // Can't compare complex numbers or NaN
        if self.is_nan() || other.is_nan() {
            return false;
        }
        if matches!(self, NumericValue::Complex(_)) || matches!(other, NumericValue::Complex(_)) {
            return false;
        }

        // Convert both to comparable form (BigRational or f64)
        match (self, other) {
            // If either is Real, use float comparison
            (NumericValue::Real(_), _) | (_, NumericValue::Real(_)) => {
                let a = self.to_f64();
                let b = other.to_f64();
                a < b
            }
            // Both exact - use rational comparison
            _ => {
                let a = self.to_rational();
                let b = other.to_rational();
                a < b
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
            Value::Complex(_, _) => {
                // Complex numbers are not ordered, so they can't be compared with <, >, etc.
                // Only = works with complex numbers
                Err(EvalError::TypeError(
                    "Cannot order complex numbers (use = for equality)".to_string(),
                ))
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
            // Check for NaN - comparisons with NaN always return #f
            if Self::is_nan(&args[i]) || Self::is_nan(&args[i + 1]) {
                return Ok(Value::Boolean(false));
            }

            // Handle infinity comparisons using NumericValue which supports infinity
            let has_infinity = match (&args[i], &args[i + 1]) {
                (Value::Real(a), _) if a.is_infinite() => true,
                (_, Value::Real(b)) if b.is_infinite() => true,
                _ => false,
            };

            if has_infinity {
                // Use NumericValue comparison which handles infinity correctly
                let a_num = NumericValue::from_value(args[i].clone())?;
                let b_num = NumericValue::from_value(args[i + 1].clone())?;

                // Convert to f64 for comparison (this handles infinity)
                let a_f64 = a_num.to_f64();
                let b_f64 = b_num.to_f64();

                // Create rationals from the f64s for the comparison function
                // For infinity, this won't work, so we do direct f64 comparison
                use num_rational::Ratio;
                let (a_rat, b_rat) = if a_f64.is_infinite() || b_f64.is_infinite() {
                    // For infinity, create dummy rationals and do direct comparison
                    let result = match op_name {
                        "<" => a_f64 < b_f64,
                        ">" => a_f64 > b_f64,
                        "<=" => a_f64 <= b_f64,
                        ">=" => a_f64 >= b_f64,
                        _ => {
                            return Err(EvalError::TypeError(format!(
                                "Unknown comparison operator: {}",
                                op_name
                            )));
                        }
                    };
                    if !result {
                        return Ok(Value::Boolean(false));
                    }
                    continue;
                } else {
                    (
                        Ratio::from_float(a_f64).unwrap(),
                        Ratio::from_float(b_f64).unwrap(),
                    )
                };

                if !op(&a_rat, &b_rat) {
                    return Ok(Value::Boolean(false));
                }
            } else {
                // Regular rational comparison (no infinity)
                let (a, _) = self.value_to_comparable(&args[i])?;
                let (b, _) = self.value_to_comparable(&args[i + 1])?;
                if !op(&a, &b) {
                    return Ok(Value::Boolean(false));
                }
            }
        }
        Ok(Value::Boolean(true))
    }

    /// Check if a value is NaN
    fn is_nan(v: &Value) -> bool {
        match v {
            Value::Real(f) => f.is_nan(),
            _ => false,
        }
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
    evaluator.check_arity_min(&args, 2, "=")?;

    // Handle complex numbers specially since they can't use value_to_comparable
    for i in 0..args.len() - 1 {
        // NaN is never equal to anything, including itself
        // Infinity can be equal to infinity
        if Evaluator::is_nan(&args[i]) || Evaluator::is_nan(&args[i + 1]) {
            return Ok(Value::Boolean(false));
        }

        let equal = match (&args[i], &args[i + 1]) {
            // If either is complex, use complex equality
            (Value::Complex(r1, i1), Value::Complex(r2, i2)) => {
                // NaN in complex numbers
                if r1.is_nan() || i1.is_nan() || r2.is_nan() || i2.is_nan() {
                    false
                } else {
                    r1 == r2 && i1 == i2
                }
            }
            (Value::Complex(r, i), other) | (other, Value::Complex(r, i)) => {
                // Complex number equals real if imaginary part is 0
                if i.is_nan() || r.is_nan() {
                    false
                } else if *i == 0.0 {
                    // Compare real part with the other number
                    match evaluator.value_to_comparable(other) {
                        Ok((rat, _)) => {
                            use num_rational::Ratio;
                            let r_rat = Ratio::from_float(*r);
                            r_rat.is_some_and(|rr| rr == rat)
                        }
                        Err(_) => false,
                    }
                } else {
                    false
                }
            }
            // Both are real numbers - use standard comparison
            (a, b) => {
                // Handle infinity specially (can't convert to rational)
                let a_is_inf = matches!(a, Value::Real(f) if f.is_infinite());
                let b_is_inf = matches!(b, Value::Real(f) if f.is_infinite());

                if a_is_inf || b_is_inf {
                    // If either is infinity, use direct f64 comparison
                    let a_f64 = NumericValue::from_value(a.clone())?.to_f64();
                    let b_f64 = NumericValue::from_value(b.clone())?.to_f64();
                    a_f64 == b_f64
                } else {
                    // Neither is infinity, use rational comparison
                    let (a_rat, _) = evaluator.value_to_comparable(a)?;
                    let (b_rat, _) = evaluator.value_to_comparable(b)?;
                    a_rat == b_rat
                }
            }
        };

        if !equal {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
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

// ========== Integer Extraction Helpers ==========

/// Extract BigInt from a Value, including inexact integers like -4.0
/// Returns (bigint_value, is_inexact)
fn extract_bigint(v: &Value, op_name: &str) -> Result<(BigInt, bool), EvalError> {
    match v {
        Value::Integer(n) => Ok((BigInt::from(*n), false)),
        Value::BigInteger(n) => Ok((n.clone(), false)),
        Value::Real(r) => {
            // Check if it's an integer-valued real (like 32.0)
            if r.is_finite() && r.trunc() == *r {
                // Convert to BigInt via i64 (sufficient for most cases)
                Ok((BigInt::from(r.trunc() as i64), true))
            } else {
                Err(EvalError::TypeError(format!(
                    "{} requires integers, got non-integer real {}",
                    op_name, r
                )))
            }
        }
        _ => Err(EvalError::TypeError(format!(
            "{} requires integers, got {}",
            op_name,
            v.type_name()
        ))),
    }
}

/// Extract i64 from a Value, including inexact integers like -4.0
/// Returns (integer_value, is_inexact)
fn extract_integer(v: &Value, op_name: &str) -> Result<(i64, bool), EvalError> {
    match v {
        Value::Integer(n) => Ok((*n, false)),
        Value::BigInteger(n) => {
            // Try to convert to i64, fail if too large
            n.to_i64().map(|i| (i, false)).ok_or_else(|| {
                EvalError::TypeError(format!("{}: integer too large for operation", op_name))
            })
        }
        Value::Real(r) => {
            // Check if it's an integer-valued real (like -4.0)
            if r.is_finite() && r.trunc() == *r {
                Ok((r.trunc() as i64, true))
            } else {
                Err(EvalError::TypeError(format!(
                    "{} requires integers, got non-integer real {}",
                    op_name, r
                )))
            }
        }
        _ => Err(EvalError::TypeError(format!(
            "{} requires integers, got {}",
            op_name,
            v.type_name()
        ))),
    }
}

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
    // Handle exact integers first (fast path)
    match (a, b) {
        (Value::Integer(a_val), Value::Integer(b_val)) => {
            if check_zero && *b_val == 0 {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(Value::Integer(int_op(*a_val, *b_val)));
        }
        (Value::BigInteger(a_val), Value::BigInteger(b_val)) => {
            if check_zero && b_val.is_zero() {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(Value::BigInteger(big_op(a_val, b_val)));
        }
        (Value::Integer(a_val), Value::BigInteger(b_val)) => {
            if check_zero && b_val.is_zero() {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(Value::BigInteger(big_op(
                &NumericValue::to_bigint(*a_val),
                b_val,
            )));
        }
        (Value::BigInteger(a_val), Value::Integer(b_val)) => {
            if check_zero && *b_val == 0 {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(Value::BigInteger(big_op(
                a_val,
                &NumericValue::to_bigint(*b_val),
            )));
        }
        _ => {}
    }

    // Handle inexact integers (like -4.0)
    let (a_int, a_inexact) = extract_integer(a, op_name)?;
    let (b_int, b_inexact) = extract_integer(b, op_name)?;

    if check_zero && b_int == 0 {
        return Err(EvalError::DivisionByZero);
    }

    let result = int_op(a_int, b_int);

    // If either operand was inexact, result is inexact
    if a_inexact || b_inexact {
        Ok(Value::Real(result as f64))
    } else {
        Ok(Value::Integer(result))
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
    evaluator.check_arity_exact(&args, 2, "modulo")?;
    // R7RS modulo: result has the same sign as the divisor
    // This is different from Rust's rem_euclid which always returns non-negative
    fn r7rs_modulo_i64(a: i64, b: i64) -> i64 {
        let rem = a % b;
        if rem == 0 || (rem > 0) == (b > 0) {
            rem
        } else {
            rem + b
        }
    }
    fn r7rs_modulo_bigint(a: &BigInt, b: &BigInt) -> BigInt {
        let rem = a % b;
        if rem.is_zero() || (rem > BigInt::from(0)) == (b > &BigInt::from(0)) {
            rem
        } else {
            rem + b
        }
    }
    binary_int_op(
        &args[0],
        &args[1],
        "modulo",
        r7rs_modulo_i64,
        r7rs_modulo_bigint,
        true, // check for division by zero
    )
}

// ============================================================================
// R7RS floor/ and truncate/ division procedures (Section 6.2.6)
// ============================================================================

/// Helper for binary division operations returning two values (quotient and remainder).
/// Uses the same pattern as binary_int_op: exact integers first, then inexact.
fn binary_div_op<FInt, FBig>(
    a: &Value,
    b: &Value,
    op_name: &str,
    int_op: FInt,
    big_op: FBig,
) -> Result<Value, EvalError>
where
    FInt: Fn(i64, i64) -> (i64, i64),
    FBig: Fn(&BigInt, &BigInt) -> (BigInt, BigInt),
{
    // Handle exact integers first (fast path)
    match (a, b) {
        (Value::Integer(a_val), Value::Integer(b_val)) => {
            if *b_val == 0 {
                return Err(EvalError::DivisionByZero);
            }
            let (q, r) = int_op(*a_val, *b_val);
            return Ok(Value::Values(vec![Value::Integer(q), Value::Integer(r)]));
        }
        (Value::BigInteger(a_val), Value::BigInteger(b_val)) => {
            if b_val.is_zero() {
                return Err(EvalError::DivisionByZero);
            }
            let (q, r) = big_op(a_val, b_val);
            return Ok(Value::Values(vec![bigint_to_value(q), bigint_to_value(r)]));
        }
        (Value::Integer(a_val), Value::BigInteger(b_val)) => {
            if b_val.is_zero() {
                return Err(EvalError::DivisionByZero);
            }
            let (q, r) = big_op(&NumericValue::to_bigint(*a_val), b_val);
            return Ok(Value::Values(vec![bigint_to_value(q), bigint_to_value(r)]));
        }
        (Value::BigInteger(a_val), Value::Integer(b_val)) => {
            if *b_val == 0 {
                return Err(EvalError::DivisionByZero);
            }
            let (q, r) = big_op(a_val, &NumericValue::to_bigint(*b_val));
            return Ok(Value::Values(vec![bigint_to_value(q), bigint_to_value(r)]));
        }
        _ => {}
    }

    // Handle inexact integers (like -5.0)
    let (a_int, a_inexact) = extract_integer(a, op_name)?;
    let (b_int, b_inexact) = extract_integer(b, op_name)?;

    if b_int == 0 {
        return Err(EvalError::DivisionByZero);
    }

    let (q, r) = int_op(a_int, b_int);

    // If either operand was inexact, result is inexact
    if a_inexact || b_inexact {
        Ok(Value::Values(vec![
            Value::Real(q as f64),
            Value::Real(r as f64),
        ]))
    } else {
        Ok(Value::Values(vec![Value::Integer(q), Value::Integer(r)]))
    }
}

/// Helper for single-value division operations (quotient or remainder only).
fn binary_div_single_op<FInt, FBig>(
    a: &Value,
    b: &Value,
    op_name: &str,
    int_op: FInt,
    big_op: FBig,
) -> Result<Value, EvalError>
where
    FInt: Fn(i64, i64) -> i64,
    FBig: Fn(&BigInt, &BigInt) -> BigInt,
{
    // Handle exact integers first (fast path)
    match (a, b) {
        (Value::Integer(a_val), Value::Integer(b_val)) => {
            if *b_val == 0 {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(Value::Integer(int_op(*a_val, *b_val)));
        }
        (Value::BigInteger(a_val), Value::BigInteger(b_val)) => {
            if b_val.is_zero() {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(bigint_to_value(big_op(a_val, b_val)));
        }
        (Value::Integer(a_val), Value::BigInteger(b_val)) => {
            if b_val.is_zero() {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(bigint_to_value(big_op(
                &NumericValue::to_bigint(*a_val),
                b_val,
            )));
        }
        (Value::BigInteger(a_val), Value::Integer(b_val)) => {
            if *b_val == 0 {
                return Err(EvalError::DivisionByZero);
            }
            return Ok(bigint_to_value(big_op(
                a_val,
                &NumericValue::to_bigint(*b_val),
            )));
        }
        _ => {}
    }

    // Handle inexact integers (like -5.0)
    let (a_int, a_inexact) = extract_integer(a, op_name)?;
    let (b_int, b_inexact) = extract_integer(b, op_name)?;

    if b_int == 0 {
        return Err(EvalError::DivisionByZero);
    }

    let result = int_op(a_int, b_int);

    // If either operand was inexact, result is inexact
    if a_inexact || b_inexact {
        Ok(Value::Real(result as f64))
    } else {
        Ok(Value::Integer(result))
    }
}

/// Convert BigInt to appropriate Value (Integer if fits, otherwise BigInteger)
fn bigint_to_value(n: BigInt) -> Value {
    match n.to_i64() {
        Some(i) => Value::Integer(i),
        None => Value::BigInteger(n),
    }
}

// Floor division helpers for i64
fn floor_div_i64(a: i64, b: i64) -> (i64, i64) {
    // floor_quotient = floor(a/b)
    // Rust's / truncates toward zero, so we need to adjust for floor semantics
    // when the signs differ and there's a non-zero remainder
    let q = if (a < 0) != (b < 0) && a % b != 0 {
        a / b - 1
    } else {
        a / b
    };
    let r = a - b * q;
    (q, r)
}

fn floor_div_bigint(a: &BigInt, b: &BigInt) -> (BigInt, BigInt) {
    let q = a / b;
    let r = a % b;
    // Adjust quotient for floor semantics if signs differ and there's a remainder
    if (a.sign() != b.sign()) && !r.is_zero() {
        let q = q - 1;
        let r = a - b * &q;
        (q, r)
    } else {
        (q, r)
    }
}

fn floor_quotient_i64(a: i64, b: i64) -> i64 {
    floor_div_i64(a, b).0
}

fn floor_quotient_bigint(a: &BigInt, b: &BigInt) -> BigInt {
    floor_div_bigint(a, b).0
}

fn floor_remainder_i64(a: i64, b: i64) -> i64 {
    floor_div_i64(a, b).1
}

fn floor_remainder_bigint(a: &BigInt, b: &BigInt) -> BigInt {
    floor_div_bigint(a, b).1
}

// Truncate division helpers for i64
fn truncate_div_i64(a: i64, b: i64) -> (i64, i64) {
    let q = a / b; // Rust's / truncates toward zero
    let r = a % b;
    (q, r)
}

fn truncate_div_bigint(a: &BigInt, b: &BigInt) -> (BigInt, BigInt) {
    let q = a / b;
    let r = a % b;
    (q, r)
}

fn truncate_quotient_i64(a: i64, b: i64) -> i64 {
    a / b
}

fn truncate_quotient_bigint(a: &BigInt, b: &BigInt) -> BigInt {
    a / b
}

fn truncate_remainder_i64(a: i64, b: i64) -> i64 {
    a % b
}

fn truncate_remainder_bigint(a: &BigInt, b: &BigInt) -> BigInt {
    a % b
}

/// (floor/ n1 n2) -> quotient remainder
/// Returns two values: floor-quotient and floor-remainder
pub(super) fn floor_div(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor/")?;
    binary_div_op(
        &args[0],
        &args[1],
        "floor/",
        floor_div_i64,
        floor_div_bigint,
    )
}

/// (floor-quotient n1 n2) -> quotient
/// Returns floor(n1/n2)
pub(super) fn floor_quotient(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor-quotient")?;
    binary_div_single_op(
        &args[0],
        &args[1],
        "floor-quotient",
        floor_quotient_i64,
        floor_quotient_bigint,
    )
}

/// (floor-remainder n1 n2) -> remainder
/// Returns n1 - n2 * floor(n1/n2)
pub(super) fn floor_remainder(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor-remainder")?;
    binary_div_single_op(
        &args[0],
        &args[1],
        "floor-remainder",
        floor_remainder_i64,
        floor_remainder_bigint,
    )
}

/// (truncate/ n1 n2) -> quotient remainder
/// Returns two values: truncate-quotient and truncate-remainder
pub(super) fn truncate_div(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate/")?;
    binary_div_op(
        &args[0],
        &args[1],
        "truncate/",
        truncate_div_i64,
        truncate_div_bigint,
    )
}

/// (truncate-quotient n1 n2) -> quotient
/// Returns truncate(n1/n2)
pub(super) fn truncate_quotient(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate-quotient")?;
    binary_div_single_op(
        &args[0],
        &args[1],
        "truncate-quotient",
        truncate_quotient_i64,
        truncate_quotient_bigint,
    )
}

/// (truncate-remainder n1 n2) -> remainder
/// Returns n1 - n2 * truncate(n1/n2)
pub(super) fn truncate_remainder(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate-remainder")?;
    binary_div_single_op(
        &args[0],
        &args[1],
        "truncate-remainder",
        truncate_remainder_i64,
        truncate_remainder_bigint,
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

    let mut max_num = NumericValue::from_value(args[0].clone()).map_err(|_| {
        EvalError::TypeError(format!("max expects numbers, got {}", args[0].type_name()))
    })?;

    // Track if we've seen any inexact number (for inexact contagion)
    let mut has_inexact = max_num.is_inexact();

    for arg in &args[1..] {
        let num = NumericValue::from_value(arg.clone()).map_err(|_| {
            EvalError::TypeError(format!("max expects numbers, got {}", arg.type_name()))
        })?;

        // Track inexact contagion
        if num.is_inexact() {
            has_inexact = true;
        }

        // Skip NaN arguments (they don't affect max/min)
        if num.is_nan() {
            continue;
        }

        // If current max is NaN, replace it with first non-NaN value
        if max_num.is_nan() {
            max_num = num;
        } else if num.greater_than(&max_num) {
            // Otherwise, update max if this number is larger
            max_num = num;
        }
    }

    // Apply inexact contagion if we saw any inexact numbers
    if has_inexact && !max_num.is_inexact() {
        max_num = max_num.to_inexact();
    }

    Ok(max_num.into_value(evaluator))
}

pub(super) fn min(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "min")?;

    let mut min_num = NumericValue::from_value(args[0].clone()).map_err(|_| {
        EvalError::TypeError(format!("min expects numbers, got {}", args[0].type_name()))
    })?;

    // Track if we've seen any inexact number (for inexact contagion)
    let mut has_inexact = min_num.is_inexact();

    for arg in &args[1..] {
        let num = NumericValue::from_value(arg.clone()).map_err(|_| {
            EvalError::TypeError(format!("min expects numbers, got {}", arg.type_name()))
        })?;

        // Track inexact contagion
        if num.is_inexact() {
            has_inexact = true;
        }

        // Skip NaN arguments (they don't affect max/min)
        if num.is_nan() {
            continue;
        }

        // If current min is NaN, replace it with first non-NaN value
        if min_num.is_nan() {
            min_num = num;
        } else if num.less_than(&min_num) {
            // Otherwise, update min if this number is smaller
            min_num = num;
        }
    }

    // Apply inexact contagion if we saw any inexact numbers
    if has_inexact && !min_num.is_inexact() {
        min_num = min_num.to_inexact();
    }

    Ok(min_num.into_value(evaluator))
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

// ========== Complex Number Helpers ==========

/// Convert a Value to Complex64
/// Real numbers are converted to complex with zero imaginary part
fn value_to_complex64(v: &Value) -> Result<Complex64, EvalError> {
    match v {
        Value::Integer(n) => Ok(Complex64::new(*n as f64, 0.0)),
        Value::BigInteger(n) => Ok(Complex64::new(n.to_f64().unwrap_or(f64::INFINITY), 0.0)),
        Value::Rational(r) => Ok(Complex64::new(r.to_f64().unwrap_or(f64::INFINITY), 0.0)),
        Value::Real(f) => Ok(Complex64::new(*f, 0.0)),
        Value::Complex(r, i) => Ok(Complex64::new(*r, *i)),
        other => Err(EvalError::TypeError(format!(
            "Expected number, got {}",
            other.type_name()
        ))),
    }
}

/// Convert Complex64 back to Value
/// If imaginary part is essentially zero, return a real Value
fn complex64_to_value(c: Complex64) -> Value {
    if c.im.abs() < 1e-10 {
        Value::Real(c.re)
    } else {
        Value::Complex(c.re, c.im)
    }
}

// ========== Square Root and Exponentiation ==========

/// (sqrt x) - Square root
/// Uses Complex64 to handle all cases uniformly, including negative reals
/// R7RS branch cut: principal square root is always in right half-plane (re >= 0)
pub(super) fn sqrt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "sqrt")?;

    // Convert to complex and use num-complex's sqrt implementation
    let z = value_to_complex64(&args[0])?;

    // R7RS requires: sqrt of negative real should always return positive imaginary part
    // num-complex respects signed zeros: sqrt(-1-0i) != sqrt(-1+0i)
    // We normalize: on negative real axis, always use +0i
    let normalized = if z.re < 0.0 && z.im == 0.0 {
        Complex64::new(z.re, 0.0) // Force +0.0 imaginary part
    } else {
        z
    };

    let result = normalized.sqrt();

    Ok(complex64_to_value(result))
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
    // R7RS: If either operand is inexact, result is inexact
    if base_num.is_zero() && power_num.is_zero() {
        return if !base_num.is_exact() || !power_num.is_exact() {
            Ok(Value::Real(1.0))
        } else {
            Ok(Value::Integer(1))
        };
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
/// Supports complex numbers: sin(z) = (e^(iz) - e^(-iz)) / 2i
pub(super) fn sin(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "sin")?;
    let z = value_to_complex64(&args[0])?;
    Ok(complex64_to_value(z.sin()))
}

/// (cos x) - Cosine
/// Supports complex numbers: cos(z) = (e^(iz) + e^(-iz)) / 2
pub(super) fn cos(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "cos")?;
    let z = value_to_complex64(&args[0])?;
    Ok(complex64_to_value(z.cos()))
}

/// (tan x) - Tangent
/// Supports complex numbers: tan(z) = sin(z) / cos(z)
pub(super) fn tan(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "tan")?;
    let z = value_to_complex64(&args[0])?;
    Ok(complex64_to_value(z.tan()))
}

/// (asin x) - Arc sine
/// Supports complex numbers with branch cuts
pub(super) fn asin(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "asin")?;
    let z = value_to_complex64(&args[0])?;
    Ok(complex64_to_value(z.asin()))
}

/// (acos x) - Arc cosine
/// Supports complex numbers with branch cuts
pub(super) fn acos(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "acos")?;
    let z = value_to_complex64(&args[0])?;
    Ok(complex64_to_value(z.acos()))
}

/// (atan x [y]) - Arc tangent (one or two arguments)
/// One argument: supports complex numbers with branch cuts
/// Two arguments: real-only atan2(y, x)
pub(super) fn atan(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_range(&args, 1, 2, "atan")?;

    if args.len() == 1 {
        // One-argument form: atan(z) - supports complex
        let z = value_to_complex64(&args[0])?;
        Ok(complex64_to_value(z.atan()))
    } else {
        // Two-argument form: atan2(y, x) - real numbers only
        let y = NumericValue::from_value(args[0].clone())?;
        let x = NumericValue::from_value(args[1].clone())?;
        Ok(Value::Real(y.to_f64().atan2(x.to_f64())))
    }
}

// ========== Exponential and Logarithmic Functions ==========

/// (exp x) - e^x
/// Supports complex numbers: e^(a+bi) = e^a * (cos(b) + i*sin(b))
pub(super) fn exp(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exp")?;
    let z = value_to_complex64(&args[0])?;
    Ok(complex64_to_value(z.exp()))
}

/// (log x [base]) - Natural log or log with base
/// Supports complex numbers with branch cut on negative real axis
pub(super) fn log(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_range(&args, 1, 2, "log")?;

    if args.len() == 1 {
        // One-argument form: natural log (supports complex)
        let z = value_to_complex64(&args[0])?;
        Ok(complex64_to_value(z.ln()))
    } else {
        // Two-argument form: log with base
        // Optimization: if both arguments are not complex, use f64 for better precision
        let x_is_not_complex = !matches!(&args[0], Value::Complex(_, _));
        let base_is_not_complex = !matches!(&args[1], Value::Complex(_, _));

        if x_is_not_complex && base_is_not_complex {
            // Use f64 math for better precision
            let x_f64 = NumericValue::from_value(args[0].clone())?.to_f64();
            let base_f64 = NumericValue::from_value(args[1].clone())?.to_f64();
            Ok(Value::Real(x_f64.log(base_f64)))
        } else {
            // General complex case: log_base(x) = ln(x) / ln(base)
            let x = value_to_complex64(&args[0])?;
            let base = value_to_complex64(&args[1])?;
            let result = x.ln() / base.ln();
            Ok(complex64_to_value(result))
        }
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
    // Track if any argument is inexact (result should be inexact)
    let mut result = BigInt::from(0);
    let mut any_inexact = false;

    for arg in &args {
        let (n, is_inexact) = extract_bigint(arg, "gcd")?;
        any_inexact = any_inexact || is_inexact;
        result = bigint_gcd(&result, &n);
    }

    // Return inexact if any argument was inexact
    if any_inexact {
        Ok(Value::Real(result.to_f64().unwrap_or(f64::INFINITY)))
    } else {
        // Try to fit in i64
        match result.to_i64() {
            Some(n) => Ok(Value::Integer(n)),
            None => Ok(Value::BigInteger(result)),
        }
    }
}

/// (lcm n1 n2 ...) - Least common multiple
pub(super) fn lcm(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // R7RS: (lcm) returns 1
    if args.is_empty() {
        return Ok(Value::Integer(1));
    }

    // Convert all arguments to BigInt
    // Track if any argument is inexact (result should be inexact)
    let mut result = BigInt::from(1);
    let mut any_inexact = false;

    for arg in &args {
        let (n, is_inexact) = extract_bigint(arg, "lcm")?;
        any_inexact = any_inexact || is_inexact;

        // LCM(a, b) = |a * b| / GCD(a, b)
        // But we need to handle zero specially
        if n.is_zero() {
            return if any_inexact {
                Ok(Value::Real(0.0))
            } else {
                Ok(Value::Integer(0))
            };
        }

        let gcd_val = bigint_gcd(&result, &n);
        result = (result * n).abs() / gcd_val;
    }

    // Return inexact if any argument was inexact
    if any_inexact {
        Ok(Value::Real(result.to_f64().unwrap_or(f64::INFINITY)))
    } else {
        // Try to fit in i64
        match result.to_i64() {
            Some(n) => Ok(Value::Integer(n)),
            None => Ok(Value::BigInteger(result)),
        }
    }
}

// ========== Rational Number Accessors ==========

/// (numerator q) - Returns the numerator of rational q
/// R7RS: If the argument is inexact, the result is also inexact.
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
        Value::Real(f) => {
            // R7RS: inexact rationals return inexact results
            use num_traits::FromPrimitive;

            if f.is_nan() || f.is_infinite() {
                return Err(EvalError::TypeError(
                    "numerator: cannot compute numerator of NaN or infinity".to_string(),
                ));
            }

            // Convert to rational to get numerator
            match BigRational::from_f64(*f) {
                Some(ratio) => {
                    // Return as inexact (Real)
                    let numer = ratio.numer();
                    Ok(Value::Real(numer.to_f64().unwrap_or(f64::INFINITY)))
                }
                None => Err(EvalError::TypeError(
                    "numerator: cannot compute numerator of this real number".to_string(),
                )),
            }
        }
        other => Err(EvalError::TypeError(format!(
            "numerator expects a rational number, got {}",
            other.type_name()
        ))),
    }
}

/// (denominator q) - Returns the denominator of rational q
/// R7RS: If the argument is inexact, the result is also inexact.
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
        Value::Real(f) => {
            // R7RS: inexact rationals return inexact results
            use num_traits::FromPrimitive;

            if f.is_nan() || f.is_infinite() {
                return Err(EvalError::TypeError(
                    "denominator: cannot compute denominator of NaN or infinity".to_string(),
                ));
            }

            // Convert to rational to get denominator
            match BigRational::from_f64(*f) {
                Some(ratio) => {
                    // Return as inexact (Real)
                    let denom = ratio.denom();
                    Ok(Value::Real(denom.to_f64().unwrap_or(f64::INFINITY)))
                }
                None => Err(EvalError::TypeError(
                    "denominator: cannot compute denominator of this real number".to_string(),
                )),
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
        Value::Complex(r, i) => Ok(Value::Complex(*r, *i)),

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

    // Floor division (floor/, floor-quotient, floor-remainder)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "floor/",
        Arity::Exact(2),
        "Returns two values: floor-quotient and floor-remainder.",
        |eval, args, _tail| floor_div(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "floor-quotient",
        Arity::Exact(2),
        "Returns floor(n1/n2).",
        |eval, args, _tail| floor_quotient(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "floor-remainder",
        Arity::Exact(2),
        "Returns n1 - n2 * floor(n1/n2).",
        |eval, args, _tail| floor_remainder(eval, args).map(EvalResult::Value),
    ));

    // Truncate division (truncate/, truncate-quotient, truncate-remainder)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "truncate/",
        Arity::Exact(2),
        "Returns two values: truncate-quotient and truncate-remainder.",
        |eval, args, _tail| truncate_div(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "truncate-quotient",
        Arity::Exact(2),
        "Returns truncate(n1/n2).",
        |eval, args, _tail| truncate_quotient(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "truncate-remainder",
        Arity::Exact(2),
        "Returns n1 - n2 * truncate(n1/n2).",
        |eval, args, _tail| truncate_remainder(eval, args).map(EvalResult::Value),
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

    // Square root (in both scheme.base and scheme.inexact per R7RS)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "sqrt",
        Arity::Exact(1),
        "Returns the principal square root of x.",
        |eval, args, _tail| sqrt(eval, args).map(EvalResult::Value),
    ));

    // Register sqrt again under scheme.inexact
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
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
        "scheme.inexact",
        "finite?",
        Arity::Exact(1),
        "Returns #t if x is finite.",
        |eval, args, _tail| finite_p(eval, args).map(EvalResult::Value),
    ));

    // Infinite predicate
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "infinite?",
        Arity::Exact(1),
        "Returns #t if x is infinite.",
        |eval, args, _tail| infinite_p(eval, args).map(EvalResult::Value),
    ));

    // NaN predicate
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "nan?",
        Arity::Exact(1),
        "Returns #t if x is NaN.",
        |eval, args, _tail| nan_p(eval, args).map(EvalResult::Value),
    ));

    // Sine
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "sin",
        Arity::Exact(1),
        "Returns the sine of x.",
        |eval, args, _tail| sin(eval, args).map(EvalResult::Value),
    ));

    // Cosine
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "cos",
        Arity::Exact(1),
        "Returns the cosine of x.",
        |eval, args, _tail| cos(eval, args).map(EvalResult::Value),
    ));

    // Tangent
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "tan",
        Arity::Exact(1),
        "Returns the tangent of x.",
        |eval, args, _tail| tan(eval, args).map(EvalResult::Value),
    ));

    // Arcsine
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "asin",
        Arity::Exact(1),
        "Returns the arcsine of x.",
        |eval, args, _tail| asin(eval, args).map(EvalResult::Value),
    ));

    // Arccosine
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "acos",
        Arity::Exact(1),
        "Returns the arccosine of x.",
        |eval, args, _tail| acos(eval, args).map(EvalResult::Value),
    ));

    // Arctangent
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "atan",
        Arity::Range(1, 2),
        "Returns the arctangent of x, or of y/x.",
        |eval, args, _tail| atan(eval, args).map(EvalResult::Value),
    ));

    // Exponential
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "exp",
        Arity::Exact(1),
        "Returns e raised to the power x.",
        |eval, args, _tail| exp(eval, args).map(EvalResult::Value),
    ));

    // Natural logarithm
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
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

    // Real part (also in scheme.base for convenience)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "real-part",
        Arity::Exact(1),
        "Returns the real part of z.",
        |eval, args, _tail| real_part(eval, args).map(EvalResult::Value),
    ));
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "real-part",
        Arity::Exact(1),
        "Returns the real part of z.",
        |eval, args, _tail| real_part(eval, args).map(EvalResult::Value),
    ));

    // Imaginary part (also in scheme.base for convenience)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "imag-part",
        Arity::Exact(1),
        "Returns the imaginary part of z.",
        |eval, args, _tail| imag_part(eval, args).map(EvalResult::Value),
    ));
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "imag-part",
        Arity::Exact(1),
        "Returns the imaginary part of z.",
        |eval, args, _tail| imag_part(eval, args).map(EvalResult::Value),
    ));

    // Magnitude
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "magnitude",
        Arity::Exact(1),
        "Returns the magnitude of z.",
        |eval, args, _tail| magnitude(eval, args).map(EvalResult::Value),
    ));

    // Angle
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "angle",
        Arity::Exact(1),
        "Returns the angle of z.",
        |eval, args, _tail| angle(eval, args).map(EvalResult::Value),
    ));

    // Make rectangular
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "make-rectangular",
        Arity::Exact(2),
        "Returns a complex number with real part x1 and imaginary part x2.",
        |eval, args, _tail| make_rectangular(eval, args).map(EvalResult::Value),
    ));

    // Make polar
    registry.register(PrimitiveFn::new(
        "scheme.complex",
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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to extract two values from Value::Values
    fn extract_two_values(v: Value) -> (Value, Value) {
        match v {
            Value::Values(vals) if vals.len() == 2 => (vals[0].clone(), vals[1].clone()),
            _ => panic!("Expected two values, got {:?}", v),
        }
    }

    // =========================================================================
    // floor/ tests - R7RS examples from spec
    // =========================================================================

    #[test]
    fn test_floor_div_positive_positive() {
        // (floor/ 5 2) => 2 1
        let (q, r) = floor_div_i64(5, 2);
        assert_eq!(q, 2);
        assert_eq!(r, 1);
    }

    #[test]
    fn test_floor_div_negative_positive() {
        // (floor/ -5 2) => -3 1
        let (q, r) = floor_div_i64(-5, 2);
        assert_eq!(q, -3);
        assert_eq!(r, 1);
    }

    #[test]
    fn test_floor_div_positive_negative() {
        // (floor/ 5 -2) => -3 -1
        let (q, r) = floor_div_i64(5, -2);
        assert_eq!(q, -3);
        assert_eq!(r, -1);
    }

    #[test]
    fn test_floor_div_negative_negative() {
        // (floor/ -5 -2) => 2 -1
        let (q, r) = floor_div_i64(-5, -2);
        assert_eq!(q, 2);
        assert_eq!(r, -1);
    }

    // =========================================================================
    // truncate/ tests - R7RS examples from spec
    // =========================================================================

    #[test]
    fn test_truncate_div_positive_positive() {
        // (truncate/ 5 2) => 2 1
        let (q, r) = truncate_div_i64(5, 2);
        assert_eq!(q, 2);
        assert_eq!(r, 1);
    }

    #[test]
    fn test_truncate_div_negative_positive() {
        // (truncate/ -5 2) => -2 -1
        let (q, r) = truncate_div_i64(-5, 2);
        assert_eq!(q, -2);
        assert_eq!(r, -1);
    }

    #[test]
    fn test_truncate_div_positive_negative() {
        // (truncate/ 5 -2) => -2 1
        let (q, r) = truncate_div_i64(5, -2);
        assert_eq!(q, -2);
        assert_eq!(r, 1);
    }

    #[test]
    fn test_truncate_div_negative_negative() {
        // (truncate/ -5 -2) => 2 -1
        let (q, r) = truncate_div_i64(-5, -2);
        assert_eq!(q, 2);
        assert_eq!(r, -1);
    }

    // =========================================================================
    // Invariant tests: n1 = n2 * quotient + remainder
    // =========================================================================

    #[test]
    fn test_floor_div_invariant() {
        for a in [-10, -5, -1, 0, 1, 5, 10] {
            for b in [-3, -2, -1, 1, 2, 3] {
                let (q, r) = floor_div_i64(a, b);
                assert_eq!(a, b * q + r, "floor/ invariant failed for ({}, {})", a, b);
            }
        }
    }

    #[test]
    fn test_truncate_div_invariant() {
        for a in [-10, -5, -1, 0, 1, 5, 10] {
            for b in [-3, -2, -1, 1, 2, 3] {
                let (q, r) = truncate_div_i64(a, b);
                assert_eq!(
                    a,
                    b * q + r,
                    "truncate/ invariant failed for ({}, {})",
                    a,
                    b
                );
            }
        }
    }

    // =========================================================================
    // Inexact integer tests (e.g., 5.0, -5.0)
    // =========================================================================

    #[test]
    fn test_floor_div_inexact() {
        let eval = Evaluator::new();
        // (floor/ 5.0 2) => 2.0 1.0
        let result = floor_div(&eval, vec![Value::Real(5.0), Value::Integer(2)]).unwrap();
        let (q, r) = extract_two_values(result);
        assert!(
            matches!(q, Value::Real(v) if v == 2.0),
            "Expected 2.0, got {:?}",
            q
        );
        assert!(
            matches!(r, Value::Real(v) if v == 1.0),
            "Expected 1.0, got {:?}",
            r
        );
    }

    #[test]
    fn test_truncate_div_inexact() {
        let eval = Evaluator::new();
        // (truncate/ -5.0 -2) => 2.0 -1.0
        let result = truncate_div(&eval, vec![Value::Real(-5.0), Value::Integer(-2)]).unwrap();
        let (q, r) = extract_two_values(result);
        assert!(
            matches!(q, Value::Real(v) if v == 2.0),
            "Expected 2.0, got {:?}",
            q
        );
        assert!(
            matches!(r, Value::Real(v) if v == -1.0),
            "Expected -1.0, got {:?}",
            r
        );
    }

    #[test]
    fn test_floor_div_exact() {
        let eval = Evaluator::new();
        // (floor/ 5 2) => 2 1 (exact integers)
        let result = floor_div(&eval, vec![Value::Integer(5), Value::Integer(2)]).unwrap();
        let (q, r) = extract_two_values(result);
        assert!(
            matches!(q, Value::Integer(2)),
            "Expected Integer(2), got {:?}",
            q
        );
        assert!(
            matches!(r, Value::Integer(1)),
            "Expected Integer(1), got {:?}",
            r
        );
    }

    #[test]
    fn test_truncate_div_exact() {
        let eval = Evaluator::new();
        // (truncate/ -5 2) => -2 -1 (exact integers)
        let result = truncate_div(&eval, vec![Value::Integer(-5), Value::Integer(2)]).unwrap();
        let (q, r) = extract_two_values(result);
        assert!(
            matches!(q, Value::Integer(-2)),
            "Expected Integer(-2), got {:?}",
            q
        );
        assert!(
            matches!(r, Value::Integer(-1)),
            "Expected Integer(-1), got {:?}",
            r
        );
    }

    // =========================================================================
    // BigInteger tests
    // =========================================================================

    #[test]
    fn test_floor_div_bigint() {
        let a = BigInt::from(1000000000000i64);
        let b = BigInt::from(7);
        let (q, r) = floor_div_bigint(&a, &b);
        // Verify invariant
        assert_eq!(&a, &(&b * &q + &r));
    }

    #[test]
    fn test_truncate_div_bigint() {
        let a = BigInt::from(-1000000000000i64);
        let b = BigInt::from(7);
        let (q, r) = truncate_div_bigint(&a, &b);
        // Verify invariant
        assert_eq!(&a, &(&b * &q + &r));
    }
}
