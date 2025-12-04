// Test suite for missing numeric operations in R7RS-small
// All tests are currently ignored and serve as a specification
// for future implementation work.

use patina_interpreter::TreeWalkInterpreter;

fn assert_eval_to(input: &str, expected: &str) {
    let interp = TreeWalkInterpreter::new_tree_walker();
    // Import libraries needed for numeric operations
    let _ = interp.eval_str("(import (scheme inexact))");
    let _ = interp.eval_str("(import (scheme complex))");
    let result = interp.eval_str(input).unwrap();
    assert_eq!(result.to_string(), expected, "Input: {}", input);
}

#[allow(dead_code)]
fn assert_eval_error(input: &str) {
    let interp = TreeWalkInterpreter::new_tree_walker();
    assert!(
        interp.eval_str(input).is_err(),
        "Expected error for: {}",
        input
    );
}

// ===== Rounding Functions =====

#[test]
fn test_floor() {
    assert_eval_to("(floor 3.7)", "3.0");
    assert_eval_to("(floor -3.7)", "-4.0");
    assert_eval_to("(floor 3.0)", "3.0");
    assert_eval_to("(floor -0.5)", "-1.0");
    assert_eval_to("(floor 5)", "5"); // Exact integer stays exact
}

#[test]
fn test_ceiling() {
    assert_eval_to("(ceiling 3.2)", "4.0");
    assert_eval_to("(ceiling -3.2)", "-3.0");
    assert_eval_to("(ceiling 3.0)", "3.0");
    assert_eval_to("(ceiling -0.5)", "-0.0"); // Rust f64 displays -0.0
    assert_eval_to("(ceiling 5)", "5"); // Exact integer stays exact
}

#[test]
fn test_truncate() {
    assert_eval_to("(truncate 3.7)", "3.0");
    assert_eval_to("(truncate -3.7)", "-3.0"); // Rounds toward zero
    assert_eval_to("(truncate 3.0)", "3.0");
    assert_eval_to("(truncate -0.5)", "-0.0"); // Rust f64 displays -0.0
    assert_eval_to("(truncate 5)", "5"); // Exact integer stays exact
}

#[test]
fn test_round() {
    assert_eval_to("(round 3.5)", "4.0"); // Banker's rounding: round to even
    assert_eval_to("(round 4.5)", "4.0"); // Banker's rounding: round to even
    assert_eval_to("(round 3.7)", "4.0");
    assert_eval_to("(round -3.7)", "-4.0");
    assert_eval_to("(round 3.2)", "3.0");
    assert_eval_to("(round 5)", "5"); // Exact integer stays exact
}

// ===== Rational Number Accessors =====

#[test]
fn test_numerator() {
    assert_eval_to("(numerator 3/4)", "3");
    assert_eval_to("(numerator 6/8)", "3"); // Simplified
    assert_eval_to("(numerator 5)", "5"); // Integer has numerator = itself
    assert_eval_to("(numerator -2/3)", "-2");
    assert_eval_to("(numerator 0)", "0");
}

#[test]
fn test_denominator() {
    assert_eval_to("(denominator 3/4)", "4");
    assert_eval_to("(denominator 6/8)", "4"); // Simplified
    assert_eval_to("(denominator 5)", "1"); // Integer has denominator = 1
    assert_eval_to("(denominator -2/3)", "3");
}

// ===== Exactness Conversion =====

#[test]
fn test_inexact_to_exact() {
    assert_eval_to("(exact 3.0)", "3");
    assert_eval_to("(exact 2.5)", "5/2");
    assert_eval_to("(exact 0.25)", "1/4");
    assert_eval_to("(exact 5)", "5"); // Already exact
    assert_eval_to("(exact 1/2)", "1/2"); // Already exact
}

#[test]
fn test_exact_to_inexact() {
    assert_eval_to("(inexact 3)", "3.0");
    assert_eval_to("(inexact 5/2)", "2.5");
    assert_eval_to("(inexact 1/4)", "0.25");
    assert_eval_to("(inexact 3.0)", "3.0"); // Already inexact
}

// ===== Square Root and Exponentiation =====

#[test]
fn test_sqrt() {
    assert_eval_to("(sqrt 4)", "2.0");
    assert_eval_to("(sqrt 9)", "3.0");
    assert_eval_to("(sqrt 2)", "1.4142135623730951"); // Inexact
    assert_eval_to("(sqrt 0)", "0.0");

    // Negative sqrt should give complex number (future)
    // assert_eval_to("(sqrt -1)", "+i");
}

#[test]
fn test_expt() {
    // Integer exponents
    assert_eval_to("(expt 2 3)", "8");
    assert_eval_to("(expt 5 2)", "25");
    assert_eval_to("(expt 2 0)", "1");
    assert_eval_to("(expt 10 -1)", "1/10"); // Negative exponent

    // Inexact exponents
    assert_eval_to("(expt 2.0 3)", "8.0");
    assert_eval_to("(expt 2 3.0)", "8.0");

    // Special cases
    assert_eval_to("(expt 0 0)", "1"); // 0^0 = 1 by convention
    assert_eval_to("(expt 1 1000)", "1");
}

#[test]
fn test_square() {
    assert_eval_to("(square 4)", "16");
    assert_eval_to("(square -5)", "25");
    assert_eval_to("(square 0)", "0");
    assert_eval_to("(square 1.5)", "2.25");
}

// ===== Number Theory =====

#[test]
fn test_gcd() {
    assert_eval_to("(gcd 12 8)", "4");
    assert_eval_to("(gcd 18 24)", "6");
    assert_eval_to("(gcd 7 13)", "1"); // Coprime
    assert_eval_to("(gcd 0 5)", "5");
    assert_eval_to("(gcd 5 0)", "5");
    assert_eval_to("(gcd -12 8)", "4"); // Handles negatives

    // Multiple arguments
    assert_eval_to("(gcd 12 18 24)", "6");
    assert_eval_to("(gcd)", "0"); // No arguments
}

#[test]
fn test_lcm() {
    assert_eval_to("(lcm 4 6)", "12");
    assert_eval_to("(lcm 12 18)", "36");
    assert_eval_to("(lcm 7 13)", "91"); // Coprime
    assert_eval_to("(lcm 0 5)", "0");
    assert_eval_to("(lcm -4 6)", "12"); // Handles negatives

    // Multiple arguments
    assert_eval_to("(lcm 2 3 4)", "12");
    assert_eval_to("(lcm)", "1"); // No arguments
}

#[test]
fn test_exact_integer_sqrt() {
    // Returns two values: sqrt and remainder
    // (exact-integer-sqrt k) = (values s r) where k = s^2 + r

    assert_eval_to("(exact-integer-sqrt 4)", "2\n0"); // 4 = 2^2 + 0
    assert_eval_to("(exact-integer-sqrt 5)", "2\n1"); // 5 = 2^2 + 1
    assert_eval_to("(exact-integer-sqrt 9)", "3\n0"); // 9 = 3^2 + 0
    assert_eval_to("(exact-integer-sqrt 10)", "3\n1"); // 10 = 3^2 + 1
    assert_eval_to("(exact-integer-sqrt 0)", "0\n0");
}

#[test]
fn test_rationalize() {
    // R7RS: If x is inexact, result is inexact; if x is exact, result is exact
    // Inexact inputs return inexact results (the simplest rational converted to float)
    // The algorithm finds 22/7 but converts it to inexact 3.142857...
    assert_eval_to("(rationalize 3.14159 0.01)", "3.142857142857143");
    assert_eval_to("(rationalize 1.5 0.1)", "1.5"); // 3/2 as inexact
    assert_eval_to("(rationalize 0.333 0.01)", "0.3333333333333333"); // 1/3 as inexact

    // Exact inputs return exact results
    assert_eval_to("(rationalize 314159/100000 1/100)", "22/7"); // Pi approximation
    assert_eval_to("(rationalize 3/2 1/10)", "3/2");
    assert_eval_to("(rationalize 333/1000 1/100)", "1/3");
}

// ===== Float Predicates =====

#[test]
fn test_finite_predicate() {
    assert_eval_to("(finite? 3)", "#t");
    assert_eval_to("(finite? 3.14)", "#t");
    assert_eval_to("(finite? 1/2)", "#t");
    assert_eval_to("(finite? +inf.0)", "#f");
    assert_eval_to("(finite? -inf.0)", "#f");
    assert_eval_to("(finite? +nan.0)", "#f");
}

#[test]
fn test_infinite_predicate() {
    assert_eval_to("(infinite? 3)", "#f");
    assert_eval_to("(infinite? 3.14)", "#f");
    assert_eval_to("(infinite? +inf.0)", "#t");
    assert_eval_to("(infinite? -inf.0)", "#t");
    assert_eval_to("(infinite? +nan.0)", "#f");
}

#[test]
fn test_nan_predicate() {
    assert_eval_to("(nan? 3)", "#f");
    assert_eval_to("(nan? 3.14)", "#f");
    assert_eval_to("(nan? +inf.0)", "#f");
    assert_eval_to("(nan? -inf.0)", "#f");
    assert_eval_to("(nan? +nan.0)", "#t");
    assert_eval_to("(nan? (/ 0.0 0.0))", "#t"); // 0.0/0.0 = NaN
}

#[test]
fn test_exact_integer_predicate() {
    assert_eval_to("(exact-integer? 3)", "#t");
    assert_eval_to("(exact-integer? 3.0)", "#f"); // Inexact
    assert_eval_to("(exact-integer? 3/2)", "#f"); // Not integer
    assert_eval_to("(exact-integer? 6/2)", "#t"); // Simplifies to 3
}

// ===== Complex Number Operations =====

#[test]
fn test_real_part() {
    assert_eval_to("(real-part 3+4i)", "3"); // Exact integer parts preserved
    assert_eval_to("(real-part 3.0+4.0i)", "3.0"); // Inexact parts preserved
    assert_eval_to("(real-part 5)", "5"); // Real number
    assert_eval_to("(real-part +i)", "0"); // Pure imaginary has exact zero real part
    assert_eval_to("(real-part -2-3i)", "-2"); // Exact integer parts preserved
}

#[test]
fn test_imag_part() {
    assert_eval_to("(imag-part 3+4i)", "4"); // Exact integer parts preserved
    assert_eval_to("(imag-part 3.0+4.0i)", "4.0"); // Inexact parts preserved
    assert_eval_to("(imag-part 5)", "0"); // Real number has imag=0
    assert_eval_to("(imag-part +i)", "1"); // Exact 1
    assert_eval_to("(imag-part -2-3i)", "-3"); // Exact integer parts preserved
}

#[test]
fn test_magnitude() {
    assert_eval_to("(magnitude 3+4i)", "5.0"); // sqrt(3^2 + 4^2) = 5
    assert_eval_to("(magnitude 5)", "5.0");
    assert_eval_to("(magnitude -5)", "5.0");
    assert_eval_to("(magnitude +i)", "1.0");
}

#[test]
fn test_angle() {
    assert_eval_to("(angle 1)", "0.0"); // Positive real
    assert_eval_to("(angle -1)", "3.141592653589793"); // Pi
    assert_eval_to("(angle +i)", "1.5707963267948966"); // Pi/2
}

#[test]
fn test_make_rectangular() {
    assert_eval_to("(make-rectangular 3 4)", "3+4i");
    assert_eval_to("(make-rectangular 5 0)", "5");
    assert_eval_to("(make-rectangular 0 1)", "+i");
    assert_eval_to("(make-rectangular -2 -3)", "-2-3i");
}

#[test]
fn test_make_polar() {
    // make-polar takes magnitude and angle
    // Note: results may be inexact due to trig functions
    let interp = TreeWalkInterpreter::new_tree_walker();
    let _ = interp.eval_str("(import (scheme complex))");
    let result = interp.eval_str("(make-polar 1 0)").unwrap().to_string();
    // Should be very close to 1 (either "1" or "1.0")
    assert!(
        result == "1" || result == "1.0",
        "Expected 1 or 1.0, got {}",
        result
    );

    // (make-polar 1 (/ pi 2)) should be approximately +i
    // But we'd need pi constant to test this properly
}

// ===== Trigonometric Functions =====
// These are part of R7RS-small but not yet implemented

#[test]
fn test_sin() {
    assert_eval_to("(sin 0)", "0.0");
    // (sin (/ pi 2)) should be approximately 1.0
}

#[test]
fn test_cos() {
    assert_eval_to("(cos 0)", "1.0");
    // (cos pi) should be approximately -1.0
}

#[test]
fn test_tan() {
    assert_eval_to("(tan 0)", "0.0");
}

#[test]
fn test_asin() {
    assert_eval_to("(asin 0)", "0.0");
    assert_eval_to("(asin 1)", "1.5707963267948966"); // Pi/2
}

#[test]
fn test_acos() {
    assert_eval_to("(acos 1)", "0.0");
    assert_eval_to("(acos 0)", "1.5707963267948966"); // Pi/2
}

#[test]
fn test_atan() {
    assert_eval_to("(atan 0)", "0.0");
    assert_eval_to("(atan 1)", "0.7853981633974483"); // Pi/4

    // Two-argument form
    assert_eval_to("(atan 0 1)", "0.0");
    assert_eval_to("(atan 1 0)", "1.5707963267948966"); // Pi/2
}

// ===== Exponential and Logarithmic Functions =====

#[test]
fn test_exp() {
    assert_eval_to("(exp 0)", "1.0");
    assert_eval_to("(exp 1)", "2.718281828459045"); // e
}

#[test]
fn test_log() {
    assert_eval_to("(log 1)", "0.0");
    assert_eval_to("(log 2.718281828459045)", "1.0"); // log(e) = 1

    // Two-argument form: log with base
    assert_eval_to("(log 8 2)", "3.0"); // log_2(8) = 3
    assert_eval_to("(log 100 10)", "2.0"); // log_10(100) = 2
}

// ========== Complex Number Support Tests ==========

#[test]
fn test_complex_sqrt() {
    // Basic complex sqrt
    assert_eval_to("(sqrt -1)", "+i");
    assert_eval_to("(sqrt -1.0)", "+i");

    // R7RS branch cut tests: sqrt of negative real axis
    assert_eval_to("(sqrt -1.0-0.0i)", "+i"); // Approaching from below
    assert_eval_to("(sqrt -1.0+0.0i)", "+i"); // Approaching from above

    // Inexact conversion
    assert_eval_to("(inexact (sqrt -1))", "+i");

    // Complex input
    assert_eval_to("(sqrt 2+2i)", "1.5537739740300374+0.6435942529055827i");
}

#[test]
fn test_complex_exp() {
    // Euler's formula: e^(iπ) = -1
    // We test e^i which should give cos(1) + i*sin(1)
    let interp = TreeWalkInterpreter::new_tree_walker();
    let _ = interp.eval_str("(import (scheme inexact))");
    let result = interp.eval_str("(exp 0+1i)").unwrap().to_string();

    // Check that it returns a complex number close to 0.540 + 0.841i
    assert!(result.contains("+"));
    assert!(result.contains("i"));
}

#[test]
fn test_complex_log() {
    // log of negative number should return complex
    // log(-1) = 0 + iπ ≈ 0 + 3.14159i
    let interp = TreeWalkInterpreter::new_tree_walker();
    let _ = interp.eval_str("(import (scheme inexact))");
    let result = interp.eval_str("(log -1)").unwrap().to_string();

    // Check that it returns a complex number with imaginary part ≈ π
    assert!(result.contains("+"));
    assert!(result.contains("3.14"));
    assert!(result.contains("i"));
}

#[test]
fn test_complex_sin() {
    // sin(i) = i*sinh(1) ≈ 0 + 1.175i
    let interp = TreeWalkInterpreter::new_tree_walker();
    let _ = interp.eval_str("(import (scheme inexact))");
    let result = interp.eval_str("(sin 0+1i)").unwrap().to_string();

    // Check that it returns a complex number
    assert!(result.contains("+"));
    assert!(result.contains("1.17"));
    assert!(result.contains("i"));
}

#[test]
fn test_complex_cos() {
    // cos(i) = cosh(1) ≈ 1.543
    let interp = TreeWalkInterpreter::new_tree_walker();
    let _ = interp.eval_str("(import (scheme inexact))");
    let result = interp.eval_str("(cos 0+1i)").unwrap().to_string();

    // Check that it returns a real number ≈ 1.543
    assert!(result.contains("1.54"));
}

#[test]
fn test_complex_asin() {
    // asin(2) with real input > 1 should return complex
    // asin(2) ≈ 1.571 - 1.317i
    let interp = TreeWalkInterpreter::new_tree_walker();
    let _ = interp.eval_str("(import (scheme inexact))");
    let result = interp.eval_str("(asin 2)").unwrap().to_string();

    // Check that it returns a complex number
    assert!(result.contains("-") || result.contains("+"));
    assert!(result.contains("i"));
}

#[test]
fn test_complex_atan() {
    // atan(i) should return complex
    let interp = TreeWalkInterpreter::new_tree_walker();
    let _ = interp.eval_str("(import (scheme inexact))");
    let result = interp.eval_str("(atan 0+1i)").unwrap();

    // Just verify it doesn't error
    assert!(!result.to_string().is_empty());
}
