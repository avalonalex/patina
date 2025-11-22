//! Tests for numeric edge cases: NaN, infinity, complex numbers
//! These ensure R7RS compliance for special numeric values

use crate::common::*;

#[test]
fn test_nan_comparisons() {
    // NaN comparisons always return #f
    assert_eval_to("(< +nan.0 0)", "#f");
    assert_eval_to("(> +nan.0 0)", "#f");
    assert_eval_to("(<= +nan.0 0)", "#f");
    assert_eval_to("(>= +nan.0 0)", "#f");
    assert_eval_to("(= +nan.0 0)", "#f");

    // NaN is not equal to itself
    assert_eval_to("(= +nan.0 +nan.0)", "#f");
}

#[test]
fn test_infinity_comparisons() {
    // Infinity comparisons
    assert_eval_to("(< +inf.0 +inf.0)", "#f");
    assert_eval_to("(> +inf.0 +inf.0)", "#f");
    assert_eval_to("(= +inf.0 +inf.0)", "#t"); // infinity equals itself

    assert_eval_to("(< 100 +inf.0)", "#t");
    assert_eval_to("(> +inf.0 100)", "#t");
    assert_eval_to("(< -inf.0 0)", "#t");
    assert_eval_to("(> 0 -inf.0)", "#t");
}

#[test]
fn test_complex_equality() {
    // Complex number equality
    assert_eval_to("(= 1+2i 1+2i)", "#t");
    assert_eval_to("(= 1+2i 3+4i)", "#f");

    // Complex with zero imaginary = real
    assert_eval_to("(= 1+0i 1.0)", "#t");
    assert_eval_to("(= 1+0i 1)", "#t");
    assert_eval_to("(= 1.0 1+0i 1)", "#t");

    // Complex with non-zero imaginary != real
    assert_eval_to("(= 1+2i 1.0)", "#f");
}

#[test]
fn test_complex_ordering_returns_false() {
    // Ordering with complex numbers returns error (which gets caught as "Unknown primitive")
    // For now we just test that equality works
    // TODO: Test proper error handling for complex ordering
    assert_eval_to("(= 1+2i 1+2i)", "#t");
}

#[test]
fn test_max_with_nan() {
    // NaN handling in max: skip NaN and find max among non-NaN values
    // Result is inexact due to NaN being inexact (inexact contagion)
    assert_eval_to("(max +nan.0 5)", "5.0");
    assert_eval_to("(max 1 +nan.0 3)", "3.0");
    assert_eval_to("(max 5 +nan.0)", "5.0");
}

#[test]
fn test_max_with_infinity() {
    assert_eval_to("(max 100 +inf.0)", "+inf.0");
    assert_eval_to("(max +inf.0 100)", "+inf.0");
    assert_eval_to("(max -inf.0 0)", "0.0"); // Result is inexact
}

#[test]
fn test_max_mixed_exact_inexact() {
    // Mixed exact/inexact - result should be inexact (float)
    assert_eval_to("(max 3.9 4)", "4.0");
    assert_eval_to("(max 4 3.9)", "4.0");
    assert_eval_to("(max 5 3.9 4)", "5.0");
}

#[test]
fn test_min_with_nan() {
    // NaN handling in min: if NaN is present, keep looking for non-NaN
    // Result should be inexact (due to NaN being inexact)
    assert_eval_to("(min +nan.0 5)", "5.0");
    assert_eval_to("(min 1 +nan.0 3)", "1.0");
}

#[test]
fn test_min_with_infinity() {
    assert_eval_to("(min 100 +inf.0)", "100.0"); // Result is inexact
    assert_eval_to("(min -inf.0 0)", "-inf.0");
}

#[test]
fn test_min_mixed_exact_inexact() {
    assert_eval_to("(min 3.9 4)", "3.9");
    assert_eval_to("(min 4 3.9)", "3.9");
}
