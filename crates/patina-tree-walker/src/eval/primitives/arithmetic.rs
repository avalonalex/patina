//! Arithmetic and numeric primitive operations (R7RS Section 6.2)
//!
//! Implements the numeric tower operations including:
//! - Basic arithmetic (+, -, *, /)
//! - Numeric comparisons (=, <, >, <=, >=)
//! - Integer division (quotient, remainder, modulo)
//! - Numeric utilities (abs, max, min)
//!
//! Most operations delegate to patina_core::numeric methods on Value.

use super::super::Evaluator;
use super::super::error::EvalError;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use patina_core::numeric::{self, NumericError};
use patina_runtime::value::Value;

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
}

// ===== Helper for NumericError conversion =====

fn numeric_err(e: NumericError, op: &str) -> EvalError {
    match e {
        NumericError::NotANumber { got } => {
            EvalError::TypeError(format!("{}: expected number, got {}", op, got))
        }
        NumericError::NotReal { got } => {
            EvalError::TypeError(format!("{}: expected real number, got {}", op, got))
        }
        NumericError::NotInteger { got } => {
            EvalError::TypeError(format!("{}: expected integer, got {}", op, got))
        }
        NumericError::NotExact { got } => {
            EvalError::TypeError(format!("{}: expected exact number, got {}", op, got))
        }
        NumericError::DivisionByZero => EvalError::DivisionByZero,
        NumericError::Undefined { operation, reason } => {
            EvalError::TypeError(format!("{}: {} - {}", op, operation, reason))
        }
        NumericError::NoExactRepresentation { value } => {
            EvalError::TypeError(format!("{}: no exact representation for {}", op, value))
        }
    }
}

// ===== Public Primitive Functions =====

pub(super) fn add(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Ok(Value::Integer(0));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_add(arg).map_err(|e| numeric_err(e, "+"))?;
    }

    Ok(result)
}

pub(super) fn subtract(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    if args.len() == 1 {
        return args[0].numeric_neg().map_err(|e| numeric_err(e, "-"));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_sub(arg).map_err(|e| numeric_err(e, "-"))?;
    }

    Ok(result)
}

pub(super) fn multiply(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Ok(Value::Integer(1));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_mul(arg).map_err(|e| numeric_err(e, "*"))?;
    }

    Ok(result)
}

pub(super) fn divide(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    if args.len() == 1 {
        // (/ x) = 1/x
        return Value::Integer(1)
            .numeric_div(&args[0])
            .map_err(|e| numeric_err(e, "/"));
    }

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_div(arg).map_err(|e| numeric_err(e, "/"))?;
    }

    Ok(result)
}

pub(super) fn numeric_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "=")?;

    for i in 0..args.len() - 1 {
        let equal = args[i]
            .numeric_eq(&args[i + 1])
            .map_err(|e| numeric_err(e, "="))?;
        if !equal {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn less_than(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "<")?;

    for i in 0..args.len() - 1 {
        let lt = args[i]
            .numeric_lt(&args[i + 1])
            .map_err(|e| numeric_err(e, "<"))?;
        if !lt {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn greater_than(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, ">")?;

    for i in 0..args.len() - 1 {
        let gt = args[i]
            .numeric_gt(&args[i + 1])
            .map_err(|e| numeric_err(e, ">"))?;
        if !gt {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn less_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "<=")?;

    for i in 0..args.len() - 1 {
        let le = args[i]
            .numeric_le(&args[i + 1])
            .map_err(|e| numeric_err(e, "<="))?;
        if !le {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn greater_equal(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, ">=")?;

    for i in 0..args.len() - 1 {
        let ge = args[i]
            .numeric_ge(&args[i + 1])
            .map_err(|e| numeric_err(e, ">="))?;
        if !ge {
            return Ok(Value::Boolean(false));
        }
    }

    Ok(Value::Boolean(true))
}

pub(super) fn quotient(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "quotient")?;
    args[0]
        .numeric_quotient(&args[1])
        .map_err(|e| numeric_err(e, "quotient"))
}

pub(super) fn remainder(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "remainder")?;
    args[0]
        .numeric_remainder(&args[1])
        .map_err(|e| numeric_err(e, "remainder"))
}

pub(super) fn modulo(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "modulo")?;
    args[0]
        .numeric_modulo(&args[1])
        .map_err(|e| numeric_err(e, "modulo"))
}

/// (floor/ n1 n2) -> quotient remainder
/// Returns two values: floor-quotient and floor-remainder
pub(super) fn floor_div(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor/")?;
    let q = args[0]
        .numeric_floor_quotient(&args[1])
        .map_err(|e| numeric_err(e, "floor/"))?;
    let r = args[0]
        .numeric_floor_remainder(&args[1])
        .map_err(|e| numeric_err(e, "floor/"))?;
    Ok(Value::Values(vec![q, r]))
}

/// (floor-quotient n1 n2) -> quotient
/// Returns floor(n1/n2)
pub(super) fn floor_quotient(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor-quotient")?;
    args[0]
        .numeric_floor_quotient(&args[1])
        .map_err(|e| numeric_err(e, "floor-quotient"))
}

/// (floor-remainder n1 n2) -> remainder
/// Returns n1 - n2 * floor(n1/n2)
pub(super) fn floor_remainder(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "floor-remainder")?;
    args[0]
        .numeric_floor_remainder(&args[1])
        .map_err(|e| numeric_err(e, "floor-remainder"))
}

/// (truncate/ n1 n2) -> quotient remainder
/// Returns two values: truncate-quotient and truncate-remainder
pub(super) fn truncate_div(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate/")?;
    let q = args[0]
        .numeric_truncate_quotient(&args[1])
        .map_err(|e| numeric_err(e, "truncate/"))?;
    let r = args[0]
        .numeric_truncate_remainder(&args[1])
        .map_err(|e| numeric_err(e, "truncate/"))?;
    Ok(Value::Values(vec![q, r]))
}

/// (truncate-quotient n1 n2) -> quotient
/// Returns truncate(n1/n2)
pub(super) fn truncate_quotient(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate-quotient")?;
    args[0]
        .numeric_truncate_quotient(&args[1])
        .map_err(|e| numeric_err(e, "truncate-quotient"))
}

/// (truncate-remainder n1 n2) -> remainder
/// Returns n1 - n2 * truncate(n1/n2)
pub(super) fn truncate_remainder(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "truncate-remainder")?;
    args[0]
        .numeric_truncate_remainder(&args[1])
        .map_err(|e| numeric_err(e, "truncate-remainder"))
}

pub(super) fn abs(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "abs")?;
    args[0].numeric_abs().map_err(|e| numeric_err(e, "abs"))
}

pub(super) fn max(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "max")?;

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_max(arg).map_err(|e| numeric_err(e, "max"))?;
    }
    Ok(result)
}

pub(super) fn min(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 1, "min")?;

    let mut result = args[0].clone();
    for arg in &args[1..] {
        result = result.numeric_min(arg).map_err(|e| numeric_err(e, "min"))?;
    }
    Ok(result)
}

// ========== Rounding Functions ==========

/// (floor x) - Rounds x toward negative infinity
pub(super) fn floor(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "floor")?;
    args[0].numeric_floor().map_err(|e| numeric_err(e, "floor"))
}

/// (ceiling x) - Rounds x toward positive infinity
pub(super) fn ceiling(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "ceiling")?;
    args[0]
        .numeric_ceiling()
        .map_err(|e| numeric_err(e, "ceiling"))
}

/// (truncate x) - Rounds x toward zero
pub(super) fn truncate(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "truncate")?;
    args[0]
        .numeric_truncate()
        .map_err(|e| numeric_err(e, "truncate"))
}

/// (round x) - Rounds x to nearest integer (banker's rounding: ties round to even)
pub(super) fn round(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "round")?;
    args[0].numeric_round().map_err(|e| numeric_err(e, "round"))
}

// ========== Square Root and Exponentiation ==========

/// (sqrt x) - Square root
/// Uses Complex64 to handle all cases uniformly, including negative reals
/// R7RS branch cut: principal square root is always in right half-plane (re >= 0)
pub(super) fn sqrt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "sqrt")?;
    args[0].numeric_sqrt().map_err(|e| numeric_err(e, "sqrt"))
}

/// (square x) - Square of x
pub(super) fn square(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "square")?;
    args[0]
        .numeric_square()
        .map_err(|e| numeric_err(e, "square"))
}

/// (expt base power) - Exponentiation
pub(super) fn expt(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "expt")?;
    args[0]
        .numeric_expt(&args[1])
        .map_err(|e| numeric_err(e, "expt"))
}

// ========== Float Predicates ==========

/// (finite? x) - Returns #t if x is finite
pub(super) fn finite_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "finite?")?;
    if !args[0].is_number() {
        return Err(EvalError::TypeError(format!(
            "finite? expects a number, got {}",
            args[0].type_name()
        )));
    }
    Ok(Value::Boolean(args[0].is_finite()))
}

/// (infinite? x) - Returns #t if x is infinite
pub(super) fn infinite_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "infinite?")?;
    if !args[0].is_number() {
        return Err(EvalError::TypeError(format!(
            "infinite? expects a number, got {}",
            args[0].type_name()
        )));
    }
    Ok(Value::Boolean(args[0].is_infinite()))
}

/// (nan? x) - Returns #t if x is NaN
pub(super) fn nan_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "nan?")?;
    if !args[0].is_number() {
        return Err(EvalError::TypeError(format!(
            "nan? expects a number, got {}",
            args[0].type_name()
        )));
    }
    Ok(Value::Boolean(args[0].is_nan()))
}

// ========== Trigonometric Functions ==========

/// (sin x) - Sine
/// Supports complex numbers: sin(z) = (e^(iz) - e^(-iz)) / 2i
pub(super) fn sin(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "sin")?;
    args[0].numeric_sin().map_err(|e| numeric_err(e, "sin"))
}

/// (cos x) - Cosine
/// Supports complex numbers: cos(z) = (e^(iz) + e^(-iz)) / 2
pub(super) fn cos(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "cos")?;
    args[0].numeric_cos().map_err(|e| numeric_err(e, "cos"))
}

/// (tan x) - Tangent
/// Supports complex numbers: tan(z) = sin(z) / cos(z)
pub(super) fn tan(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "tan")?;
    args[0].numeric_tan().map_err(|e| numeric_err(e, "tan"))
}

/// (asin x) - Arc sine
/// Supports complex numbers with branch cuts
pub(super) fn asin(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "asin")?;
    args[0].numeric_asin().map_err(|e| numeric_err(e, "asin"))
}

/// (acos x) - Arc cosine
/// Supports complex numbers with branch cuts
pub(super) fn acos(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "acos")?;
    args[0].numeric_acos().map_err(|e| numeric_err(e, "acos"))
}

/// (atan x [y]) - Arc tangent (one or two arguments)
/// One argument: supports complex numbers with branch cuts
/// Two arguments: real-only atan2(y, x)
pub(super) fn atan(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_range(&args, 1, 2, "atan")?;

    if args.len() == 1 {
        args[0].numeric_atan().map_err(|e| numeric_err(e, "atan"))
    } else {
        args[0]
            .numeric_atan2(&args[1])
            .map_err(|e| numeric_err(e, "atan"))
    }
}

// ========== Exponential and Logarithmic Functions ==========

/// (exp x) - e^x
/// Supports complex numbers: e^(a+bi) = e^a * (cos(b) + i*sin(b))
pub(super) fn exp(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exp")?;
    args[0].numeric_exp().map_err(|e| numeric_err(e, "exp"))
}

/// (log x [base]) - Natural log or log with base
/// Supports complex numbers with branch cut on negative real axis
pub(super) fn log(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_range(&args, 1, 2, "log")?;

    if args.len() == 1 {
        args[0].numeric_log().map_err(|e| numeric_err(e, "log"))
    } else {
        args[0]
            .numeric_log_base(&args[1])
            .map_err(|e| numeric_err(e, "log"))
    }
}

// ========== Number Theory Functions ==========

/// (gcd n1 n2 ...) - Greatest common divisor
pub(super) fn gcd(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    numeric::numeric_gcd_many(&args).map_err(|e| numeric_err(e, "gcd"))
}

/// (lcm n1 n2 ...) - Least common multiple
pub(super) fn lcm(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    numeric::numeric_lcm_many(&args).map_err(|e| numeric_err(e, "lcm"))
}

// ========== Rational Number Accessors ==========

/// (numerator q) - Returns the numerator of rational q
/// R7RS: If the argument is inexact, the result is also inexact.
pub(super) fn numerator(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "numerator")?;
    args[0].numerator().map_err(|e| numeric_err(e, "numerator"))
}

/// (denominator q) - Returns the denominator of rational q
/// R7RS: If the argument is inexact, the result is also inexact.
pub(super) fn denominator(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "denominator")?;
    args[0]
        .denominator()
        .map_err(|e| numeric_err(e, "denominator"))
}

// ========== Exactness Conversion ==========

/// (exact z) - Convert to exact representation (inexact->exact)
pub(super) fn exact(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "exact")?;
    args[0].to_exact().map_err(|e| numeric_err(e, "exact"))
}

/// (inexact z) - Convert to inexact representation (exact->inexact)
pub(super) fn inexact(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "inexact")?;
    Ok(args[0].to_inexact())
}

// ========== Complex Number Operations ==========

/// (real-part z) - Real part of complex number
/// IMPORTANT: Returns the real part preserving its exactness
pub(super) fn real_part(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "real-part")?;
    args[0].real_part().map_err(|e| numeric_err(e, "real-part"))
}

/// (imag-part z) - Imaginary part of complex number
/// IMPORTANT: Returns the imaginary part preserving its exactness
pub(super) fn imag_part(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "imag-part")?;
    args[0].imag_part().map_err(|e| numeric_err(e, "imag-part"))
}

/// (magnitude z) - Magnitude of complex number
pub(super) fn magnitude(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "magnitude")?;
    args[0].magnitude().map_err(|e| numeric_err(e, "magnitude"))
}

/// (angle z) - Angle of complex number in radians
pub(super) fn angle(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 1, "angle")?;
    args[0].angle().map_err(|e| numeric_err(e, "angle"))
}

/// (make-rectangular real imag) - Construct complex number from real and imaginary parts
/// Preserves exactness of the arguments
pub(super) fn make_rectangular(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "make-rectangular")?;
    numeric::make_rectangular(&args[0], &args[1]).map_err(|e| numeric_err(e, "make-rectangular"))
}

/// (make-polar magnitude angle) - Construct complex number from polar coordinates
/// Always produces inexact results since trig functions are inexact
pub(super) fn make_polar(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "make-polar")?;
    numeric::make_polar(&args[0], &args[1]).map_err(|e| numeric_err(e, "make-polar"))
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
///
/// R7RS: If x is inexact, the result is inexact. If x is exact, the result is exact.
pub(super) fn rationalize(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    _eval.check_arity_exact(&args, 2, "rationalize")?;

    let x = &args[0];
    let tolerance = &args[1];

    // Validate inputs are real numbers
    if !x.is_number() || matches!(x, Value::Complex(_)) {
        return Err(EvalError::TypeError(
            "rationalize: expected real number".to_string(),
        ));
    }
    if !tolerance.is_number() || matches!(tolerance, Value::Complex(_)) {
        return Err(EvalError::TypeError(
            "rationalize: expected real number for tolerance".to_string(),
        ));
    }

    // Check if x is inexact - this determines the exactness of the result
    let x_is_inexact = x.is_inexact();

    // Convert to f64 for the algorithm
    let x_f64 = match x {
        Value::Integer(n) => *n as f64,
        Value::BigInteger(n) => n.to_f64().unwrap_or(f64::INFINITY),
        Value::Rational(r) => r.to_f64().unwrap_or(f64::INFINITY),
        Value::Real(f) => *f,
        _ => unreachable!(),
    };

    let tol_f64 = match tolerance {
        Value::Integer(n) => (*n as f64).abs(),
        Value::BigInteger(n) => n.to_f64().unwrap_or(f64::INFINITY).abs(),
        Value::Rational(r) => r.to_f64().unwrap_or(f64::INFINITY).abs(),
        Value::Real(f) => f.abs(),
        _ => unreachable!(),
    };

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

    // R7RS: If x is inexact, return inexact result; otherwise exact
    if x_is_inexact {
        // Convert rational to inexact (f64)
        let f = result.to_f64().unwrap_or(f64::NAN);
        Ok(Value::Real(f))
    } else {
        // Return exact rational
        Ok(_eval.rational_to_value(result))
    }
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
    // floor/ and truncate/ public primitive tests
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
}
