//! Arithmetic and numeric primitive operations (R7RS Section 6.2)
//!
//! This module implements the numeric tower operations including:
//! - Basic arithmetic (+, -, *, /)
//! - Numeric comparisons (=, <, >, <=, >=)
//! - Integer division (quotient, remainder, modulo, floor/, truncate/)
//! - Rounding (floor, ceiling, truncate, round)
//! - Numeric utilities (abs, max, min)
//! - Transcendental functions (sqrt, sin, cos, exp, log, etc.)
//! - Float predicates (finite?, infinite?, nan?)
//! - Complex number operations (real-part, imag-part, magnitude, etc.)
//! - Number theory (gcd, lcm, numerator, denominator, etc.)
//!
//! Most operations delegate to patina_core::numeric methods on Value.

mod basic;
mod comparison;
mod complex;
mod division;
mod helpers;
mod number_theory;
mod predicates;
mod rounding;
mod transcendental;

/// Register arithmetic primitives with the registry
///
/// Registers (scheme base), (scheme inexact), and (scheme complex)
/// arithmetic primitives with their full namespace.
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // ===== Basic Arithmetic =====

    // Addition
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "+",
        Arity::Min(0),
        "Returns the sum of its arguments. With no arguments, returns 0.",
        |eval, args, _tail| basic::add(eval, args).map(EvalResult::Value),
    ));

    // Subtraction
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "-",
        Arity::Min(1),
        "Subtracts subsequent arguments from the first. With one argument, returns its negation.",
        |eval, args, _tail| basic::subtract(eval, args).map(EvalResult::Value),
    ));

    // Multiplication
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "*",
        Arity::Min(0),
        "Returns the product of its arguments. With no arguments, returns 1.",
        |eval, args, _tail| basic::multiply(eval, args).map(EvalResult::Value),
    ));

    // Division
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "/",
        Arity::Min(1),
        "Divides the first argument by subsequent arguments. With one argument, returns its reciprocal.",
        |eval, args, _tail| basic::divide(eval, args).map(EvalResult::Value),
    ));

    // ===== Numeric Comparisons =====

    // Numeric equality
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "=",
        Arity::Min(2),
        "Returns #t if all arguments are numerically equal.",
        |eval, args, _tail| comparison::numeric_equal(eval, args).map(EvalResult::Value),
    ));

    // Less than
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "<",
        Arity::Min(2),
        "Returns #t if arguments are monotonically increasing.",
        |eval, args, _tail| comparison::less_than(eval, args).map(EvalResult::Value),
    ));

    // Greater than
    registry.register(PrimitiveFn::new(
        "scheme.base",
        ">",
        Arity::Min(2),
        "Returns #t if arguments are monotonically decreasing.",
        |eval, args, _tail| comparison::greater_than(eval, args).map(EvalResult::Value),
    ));

    // Less than or equal
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "<=",
        Arity::Min(2),
        "Returns #t if arguments are monotonically non-decreasing.",
        |eval, args, _tail| comparison::less_equal(eval, args).map(EvalResult::Value),
    ));

    // Greater than or equal
    registry.register(PrimitiveFn::new(
        "scheme.base",
        ">=",
        Arity::Min(2),
        "Returns #t if arguments are monotonically non-increasing.",
        |eval, args, _tail| comparison::greater_equal(eval, args).map(EvalResult::Value),
    ));

    // ===== Integer Division =====

    // Quotient
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "quotient",
        Arity::Exact(2),
        "Returns the quotient of dividing n1 by n2.",
        |eval, args, _tail| division::quotient(eval, args).map(EvalResult::Value),
    ));

    // Remainder
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "remainder",
        Arity::Exact(2),
        "Returns the remainder of dividing n1 by n2.",
        |eval, args, _tail| division::remainder(eval, args).map(EvalResult::Value),
    ));

    // Modulo
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "modulo",
        Arity::Exact(2),
        "Returns n1 modulo n2.",
        |eval, args, _tail| division::modulo(eval, args).map(EvalResult::Value),
    ));

    // Floor division (floor/, floor-quotient, floor-remainder)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "floor/",
        Arity::Exact(2),
        "Returns two values: floor-quotient and floor-remainder.",
        |eval, args, _tail| division::floor_div(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "floor-quotient",
        Arity::Exact(2),
        "Returns floor(n1/n2).",
        |eval, args, _tail| division::floor_quotient(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "floor-remainder",
        Arity::Exact(2),
        "Returns n1 - n2 * floor(n1/n2).",
        |eval, args, _tail| division::floor_remainder(eval, args).map(EvalResult::Value),
    ));

    // Truncate division (truncate/, truncate-quotient, truncate-remainder)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "truncate/",
        Arity::Exact(2),
        "Returns two values: truncate-quotient and truncate-remainder.",
        |eval, args, _tail| division::truncate_div(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "truncate-quotient",
        Arity::Exact(2),
        "Returns truncate(n1/n2).",
        |eval, args, _tail| division::truncate_quotient(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "truncate-remainder",
        Arity::Exact(2),
        "Returns n1 - n2 * truncate(n1/n2).",
        |eval, args, _tail| division::truncate_remainder(eval, args).map(EvalResult::Value),
    ));

    // ===== Rounding and Utilities =====

    // Floor
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "floor",
        Arity::Exact(1),
        "Returns the largest integer not larger than x.",
        |eval, args, _tail| rounding::floor(eval, args).map(EvalResult::Value),
    ));

    // Ceiling
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "ceiling",
        Arity::Exact(1),
        "Returns the smallest integer not smaller than x.",
        |eval, args, _tail| rounding::ceiling(eval, args).map(EvalResult::Value),
    ));

    // Truncate
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "truncate",
        Arity::Exact(1),
        "Returns the integer closest to x whose absolute value is not larger than the absolute value of x.",
        |eval, args, _tail| rounding::truncate(eval, args).map(EvalResult::Value),
    ));

    // Round
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "round",
        Arity::Exact(1),
        "Returns the closest integer to x, rounding to even when x is halfway between two integers.",
        |eval, args, _tail| rounding::round(eval, args).map(EvalResult::Value),
    ));

    // Absolute value
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "abs",
        Arity::Exact(1),
        "Returns the absolute value of x.",
        |eval, args, _tail| rounding::abs(eval, args).map(EvalResult::Value),
    ));

    // Maximum
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "max",
        Arity::Min(1),
        "Returns the maximum of its arguments.",
        |eval, args, _tail| rounding::max(eval, args).map(EvalResult::Value),
    ));

    // Minimum
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "min",
        Arity::Min(1),
        "Returns the minimum of its arguments.",
        |eval, args, _tail| rounding::min(eval, args).map(EvalResult::Value),
    ));

    // ===== Transcendental Functions =====

    // Square root (in both scheme.base and scheme.inexact per R7RS)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "sqrt",
        Arity::Exact(1),
        "Returns the principal square root of x.",
        |eval, args, _tail| transcendental::sqrt(eval, args).map(EvalResult::Value),
    ));

    // Register sqrt again under scheme.inexact
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "sqrt",
        Arity::Exact(1),
        "Returns the principal square root of x.",
        |eval, args, _tail| transcendental::sqrt(eval, args).map(EvalResult::Value),
    ));

    // Square
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "square",
        Arity::Exact(1),
        "Returns the square of x.",
        |eval, args, _tail| transcendental::square(eval, args).map(EvalResult::Value),
    ));

    // Exponentiation
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "expt",
        Arity::Exact(2),
        "Returns z1 raised to the power z2.",
        |eval, args, _tail| transcendental::expt(eval, args).map(EvalResult::Value),
    ));

    // Sine
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "sin",
        Arity::Exact(1),
        "Returns the sine of x.",
        |eval, args, _tail| transcendental::sin(eval, args).map(EvalResult::Value),
    ));

    // Cosine
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "cos",
        Arity::Exact(1),
        "Returns the cosine of x.",
        |eval, args, _tail| transcendental::cos(eval, args).map(EvalResult::Value),
    ));

    // Tangent
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "tan",
        Arity::Exact(1),
        "Returns the tangent of x.",
        |eval, args, _tail| transcendental::tan(eval, args).map(EvalResult::Value),
    ));

    // Arcsine
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "asin",
        Arity::Exact(1),
        "Returns the arcsine of x.",
        |eval, args, _tail| transcendental::asin(eval, args).map(EvalResult::Value),
    ));

    // Arccosine
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "acos",
        Arity::Exact(1),
        "Returns the arccosine of x.",
        |eval, args, _tail| transcendental::acos(eval, args).map(EvalResult::Value),
    ));

    // Arctangent
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "atan",
        Arity::Range(1, 2),
        "Returns the arctangent of x, or of y/x.",
        |eval, args, _tail| transcendental::atan(eval, args).map(EvalResult::Value),
    ));

    // Exponential
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "exp",
        Arity::Exact(1),
        "Returns e raised to the power x.",
        |eval, args, _tail| transcendental::exp(eval, args).map(EvalResult::Value),
    ));

    // Natural logarithm
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "log",
        Arity::Range(1, 2),
        "Returns the natural logarithm of x, or logarithm of x in base y.",
        |eval, args, _tail| transcendental::log(eval, args).map(EvalResult::Value),
    ));

    // ===== Float Predicates =====

    // Finite predicate
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "finite?",
        Arity::Exact(1),
        "Returns #t if x is finite.",
        |eval, args, _tail| predicates::finite_p(eval, args).map(EvalResult::Value),
    ));

    // Infinite predicate
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "infinite?",
        Arity::Exact(1),
        "Returns #t if x is infinite.",
        |eval, args, _tail| predicates::infinite_p(eval, args).map(EvalResult::Value),
    ));

    // NaN predicate
    registry.register(PrimitiveFn::new(
        "scheme.inexact",
        "nan?",
        Arity::Exact(1),
        "Returns #t if x is NaN.",
        |eval, args, _tail| predicates::nan_p(eval, args).map(EvalResult::Value),
    ));

    // ===== Complex Number Operations =====

    // Real part (also in scheme.base for convenience)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "real-part",
        Arity::Exact(1),
        "Returns the real part of z.",
        |eval, args, _tail| complex::real_part(eval, args).map(EvalResult::Value),
    ));
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "real-part",
        Arity::Exact(1),
        "Returns the real part of z.",
        |eval, args, _tail| complex::real_part(eval, args).map(EvalResult::Value),
    ));

    // Imaginary part (also in scheme.base for convenience)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "imag-part",
        Arity::Exact(1),
        "Returns the imaginary part of z.",
        |eval, args, _tail| complex::imag_part(eval, args).map(EvalResult::Value),
    ));
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "imag-part",
        Arity::Exact(1),
        "Returns the imaginary part of z.",
        |eval, args, _tail| complex::imag_part(eval, args).map(EvalResult::Value),
    ));

    // Magnitude
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "magnitude",
        Arity::Exact(1),
        "Returns the magnitude of z.",
        |eval, args, _tail| complex::magnitude(eval, args).map(EvalResult::Value),
    ));

    // Angle
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "angle",
        Arity::Exact(1),
        "Returns the angle of z.",
        |eval, args, _tail| complex::angle(eval, args).map(EvalResult::Value),
    ));

    // Make rectangular
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "make-rectangular",
        Arity::Exact(2),
        "Returns a complex number with real part x1 and imaginary part x2.",
        |eval, args, _tail| complex::make_rectangular(eval, args).map(EvalResult::Value),
    ));

    // Make polar
    registry.register(PrimitiveFn::new(
        "scheme.complex",
        "make-polar",
        Arity::Exact(2),
        "Returns a complex number with magnitude x1 and angle x2.",
        |eval, args, _tail| complex::make_polar(eval, args).map(EvalResult::Value),
    ));

    // ===== Number Theory =====

    // Greatest common divisor
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "gcd",
        Arity::Min(0),
        "Returns the greatest common divisor of its arguments.",
        |eval, args, _tail| number_theory::gcd(eval, args).map(EvalResult::Value),
    ));

    // Least common multiple
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "lcm",
        Arity::Min(0),
        "Returns the least common multiple of its arguments.",
        |eval, args, _tail| number_theory::lcm(eval, args).map(EvalResult::Value),
    ));

    // Numerator
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "numerator",
        Arity::Exact(1),
        "Returns the numerator of x.",
        |eval, args, _tail| number_theory::numerator(eval, args).map(EvalResult::Value),
    ));

    // Denominator
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "denominator",
        Arity::Exact(1),
        "Returns the denominator of x.",
        |eval, args, _tail| number_theory::denominator(eval, args).map(EvalResult::Value),
    ));

    // Exact
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "exact",
        Arity::Exact(1),
        "Returns an exact representation of x.",
        |eval, args, _tail| number_theory::exact(eval, args).map(EvalResult::Value),
    ));

    // Inexact
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "inexact",
        Arity::Exact(1),
        "Returns an inexact representation of x.",
        |eval, args, _tail| number_theory::inexact(eval, args).map(EvalResult::Value),
    ));

    // Exact integer square root
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "exact-integer-sqrt",
        Arity::Exact(1),
        "Returns two values s and r where k = s^2 + r and k < (s+1)^2.",
        |eval, args, _tail| number_theory::exact_integer_sqrt(eval, args).map(EvalResult::Value),
    ));

    // Rationalize
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "rationalize",
        Arity::Exact(2),
        "Returns the simplest rational number differing from x by no more than y.",
        |eval, args, _tail| number_theory::rationalize(eval, args).map(EvalResult::Value),
    ));
}

#[cfg(test)]
mod tests {
    use crate::eval::Evaluator;
    use patina_runtime::value::Value;

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
        let result =
            super::division::floor_div(&eval, vec![Value::Real(5.0), Value::Integer(2)]).unwrap();
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
        let result =
            super::division::truncate_div(&eval, vec![Value::Real(-5.0), Value::Integer(-2)])
                .unwrap();
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
        let result =
            super::division::floor_div(&eval, vec![Value::Integer(5), Value::Integer(2)]).unwrap();
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
        let result =
            super::division::truncate_div(&eval, vec![Value::Integer(-5), Value::Integer(2)])
                .unwrap();
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
