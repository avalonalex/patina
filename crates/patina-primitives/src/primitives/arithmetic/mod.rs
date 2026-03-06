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
//! All operations delegate to Heap numeric methods on TaggedValue.

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
pub(super) fn register(registry: &mut crate::registry::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // ===== Basic Arithmetic =====

    // Addition (with fast fixnum path)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "+",
        Arity::Min(0),
        "Returns the sum of its arguments. With no arguments, returns 0.",
        basic::add,
    ));

    // Subtraction (with fast fixnum path)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "-",
        Arity::Min(1),
        "Subtracts subsequent arguments from the first. With one argument, returns its negation.",
        basic::subtract,
    ));

    // Multiplication (with fast fixnum path)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "*",
        Arity::Min(0),
        "Returns the product of its arguments. With no arguments, returns 1.",
        basic::multiply,
    ));

    // Division (no fast path - typically produces rationals)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "/",
        Arity::Min(1),
        "Divides the first argument by subsequent arguments. With one argument, returns its reciprocal.",
        basic::divide,
    ));

    // ===== Numeric Comparisons (all with fast fixnum paths) =====

    // Numeric equality
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "=",
        Arity::Min(2),
        "Returns #t if all arguments are numerically equal.",
        comparison::numeric_equal,
    ));

    // Less than
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "<",
        Arity::Min(2),
        "Returns #t if arguments are monotonically increasing.",
        comparison::less_than,
    ));

    // Greater than
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        ">",
        Arity::Min(2),
        "Returns #t if arguments are monotonically decreasing.",
        comparison::greater_than,
    ));

    // Less than or equal
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "<=",
        Arity::Min(2),
        "Returns #t if arguments are monotonically non-decreasing.",
        comparison::less_equal,
    ));

    // Greater than or equal
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        ">=",
        Arity::Min(2),
        "Returns #t if arguments are monotonically non-increasing.",
        comparison::greater_equal,
    ));

    // ===== Integer Division =====

    // Quotient
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "quotient",
        Arity::Exact(2),
        "Returns the quotient of dividing n1 by n2.",
        division::quotient,
    ));

    // Remainder
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "remainder",
        Arity::Exact(2),
        "Returns the remainder of dividing n1 by n2.",
        division::remainder,
    ));

    // Modulo
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "modulo",
        Arity::Exact(2),
        "Returns n1 modulo n2.",
        division::modulo,
    ));

    // Floor division (floor/, floor-quotient, floor-remainder)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "floor/",
        Arity::Exact(2),
        "Returns two values: floor-quotient and floor-remainder.",
        division::floor_div,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "floor-quotient",
        Arity::Exact(2),
        "Returns floor(n1/n2).",
        division::floor_quotient,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "floor-remainder",
        Arity::Exact(2),
        "Returns n1 - n2 * floor(n1/n2).",
        division::floor_remainder,
    ));

    // Truncate division (truncate/, truncate-quotient, truncate-remainder)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "truncate/",
        Arity::Exact(2),
        "Returns two values: truncate-quotient and truncate-remainder.",
        division::truncate_div,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "truncate-quotient",
        Arity::Exact(2),
        "Returns truncate(n1/n2).",
        division::truncate_quotient,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "truncate-remainder",
        Arity::Exact(2),
        "Returns n1 - n2 * truncate(n1/n2).",
        division::truncate_remainder,
    ));

    // ===== Rounding and Utilities =====

    // Floor
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "floor",
        Arity::Exact(1),
        "Returns the largest integer not larger than x.",
        rounding::floor,
    ));

    // Ceiling
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "ceiling",
        Arity::Exact(1),
        "Returns the smallest integer not smaller than x.",
        rounding::ceiling,
    ));

    // Truncate
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "truncate",
        Arity::Exact(1),
        "Returns the integer closest to x whose absolute value is not larger than the absolute value of x.",
        rounding::truncate,
    ));

    // Round
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "round",
        Arity::Exact(1),
        "Returns the closest integer to x, rounding to even when x is halfway between two integers.",
        rounding::round,
    ));

    // Absolute value
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "abs",
        Arity::Exact(1),
        "Returns the absolute value of x.",
        rounding::abs,
    ));

    // Maximum
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "max",
        Arity::Min(1),
        "Returns the maximum of its arguments.",
        rounding::max,
    ));

    // Minimum
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "min",
        Arity::Min(1),
        "Returns the minimum of its arguments.",
        rounding::min,
    ));

    // ===== Transcendental Functions =====

    // Square root (in both scheme.base and scheme.inexact per R7RS)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "sqrt",
        Arity::Exact(1),
        "Returns the principal square root of x.",
        transcendental::sqrt,
    ));

    // Register sqrt again under scheme.inexact
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "sqrt",
        Arity::Exact(1),
        "Returns the principal square root of x.",
        transcendental::sqrt,
    ));

    // Square
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "square",
        Arity::Exact(1),
        "Returns the square of x.",
        transcendental::square,
    ));

    // Exponentiation
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "expt",
        Arity::Exact(2),
        "Returns z1 raised to the power z2.",
        transcendental::expt,
    ));

    // Sine
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "sin",
        Arity::Exact(1),
        "Returns the sine of x.",
        transcendental::sin,
    ));

    // Cosine
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "cos",
        Arity::Exact(1),
        "Returns the cosine of x.",
        transcendental::cos,
    ));

    // Tangent
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "tan",
        Arity::Exact(1),
        "Returns the tangent of x.",
        transcendental::tan,
    ));

    // Arcsine
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "asin",
        Arity::Exact(1),
        "Returns the arcsine of x.",
        transcendental::asin,
    ));

    // Arccosine
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "acos",
        Arity::Exact(1),
        "Returns the arccosine of x.",
        transcendental::acos,
    ));

    // Arctangent
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "atan",
        Arity::Range(1, 2),
        "Returns the arctangent of x, or of y/x.",
        transcendental::atan,
    ));

    // Exponential
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "exp",
        Arity::Exact(1),
        "Returns e raised to the power x.",
        transcendental::exp,
    ));

    // Natural logarithm
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "log",
        Arity::Range(1, 2),
        "Returns the natural logarithm of x, or logarithm of x in base y.",
        transcendental::log,
    ));

    // ===== Float Predicates =====

    // Finite predicate
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "finite?",
        Arity::Exact(1),
        "Returns #t if x is finite.",
        predicates::finite_p,
    ));

    // Infinite predicate
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "infinite?",
        Arity::Exact(1),
        "Returns #t if x is infinite.",
        predicates::infinite_p,
    ));

    // NaN predicate
    registry.register(PrimitiveFn::new_heap(
        "scheme.inexact",
        "nan?",
        Arity::Exact(1),
        "Returns #t if x is NaN.",
        predicates::nan_p,
    ));

    // ===== Complex Number Operations =====

    // Real part (also in scheme.base for convenience)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "real-part",
        Arity::Exact(1),
        "Returns the real part of z.",
        complex::real_part,
    ));
    registry.register(PrimitiveFn::new_heap(
        "scheme.complex",
        "real-part",
        Arity::Exact(1),
        "Returns the real part of z.",
        complex::real_part,
    ));

    // Imaginary part (also in scheme.base for convenience)
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "imag-part",
        Arity::Exact(1),
        "Returns the imaginary part of z.",
        complex::imag_part,
    ));
    registry.register(PrimitiveFn::new_heap(
        "scheme.complex",
        "imag-part",
        Arity::Exact(1),
        "Returns the imaginary part of z.",
        complex::imag_part,
    ));

    // Magnitude
    registry.register(PrimitiveFn::new_heap(
        "scheme.complex",
        "magnitude",
        Arity::Exact(1),
        "Returns the magnitude of z.",
        complex::magnitude,
    ));

    // Angle
    registry.register(PrimitiveFn::new_heap(
        "scheme.complex",
        "angle",
        Arity::Exact(1),
        "Returns the angle of z.",
        complex::angle,
    ));

    // Make rectangular
    registry.register(PrimitiveFn::new_heap(
        "scheme.complex",
        "make-rectangular",
        Arity::Exact(2),
        "Returns a complex number with real part x1 and imaginary part x2.",
        complex::make_rectangular,
    ));

    // Make polar
    registry.register(PrimitiveFn::new_heap(
        "scheme.complex",
        "make-polar",
        Arity::Exact(2),
        "Returns a complex number with magnitude x1 and angle x2.",
        complex::make_polar,
    ));

    // ===== Number Theory =====

    // Greatest common divisor
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "gcd",
        Arity::Min(0),
        "Returns the greatest common divisor of its arguments.",
        number_theory::gcd,
    ));

    // Least common multiple
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "lcm",
        Arity::Min(0),
        "Returns the least common multiple of its arguments.",
        number_theory::lcm,
    ));

    // Numerator
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "numerator",
        Arity::Exact(1),
        "Returns the numerator of x.",
        number_theory::numerator,
    ));

    // Denominator
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "denominator",
        Arity::Exact(1),
        "Returns the denominator of x.",
        number_theory::denominator,
    ));

    // Exact
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "exact",
        Arity::Exact(1),
        "Returns an exact representation of x.",
        number_theory::exact,
    ));

    // Inexact
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "inexact",
        Arity::Exact(1),
        "Returns an inexact representation of x.",
        number_theory::inexact,
    ));

    // Exact integer square root
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "exact-integer-sqrt",
        Arity::Exact(1),
        "Returns two values s and r where k = s^2 + r and k < (s+1)^2.",
        number_theory::exact_integer_sqrt,
    ));

    // Rationalize
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "rationalize",
        Arity::Exact(2),
        "Returns the simplest rational number differing from x by no more than y.",
        number_theory::rationalize,
    ));
}
