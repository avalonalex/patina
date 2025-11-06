//! R7RS Rational Number Tests
//!
//! Comprehensive tests for rational number support including:
//! - Rational arithmetic
//! - Rational normalization and simplification
//! - Rational comparisons
//! - Exact/inexact semantics
//! - Edge cases

use super::common::*;

// ===== Rational Literals and Parsing =====

#[test]
fn test_rational_literals() {
    assert_eval_to("1/2", "1/2");
    assert_eval_to("3/4", "3/4");
    assert_eval_to("5/6", "5/6");
    assert_eval_to("100/200", "1/2"); // Auto-simplification
}

#[test]
fn test_rational_simplification() {
    assert_eval_to("2/4", "1/2");
    assert_eval_to("4/8", "1/2");
    assert_eval_to("10/20", "1/2");
    assert_eval_to("15/45", "1/3");
    assert_eval_to("21/49", "3/7");
}

#[test]
fn test_rational_to_integer_simplification() {
    assert_eval_to("6/3", "2");
    assert_eval_to("10/5", "2");
    assert_eval_to("15/3", "5");
    assert_eval_to("100/10", "10");
}

#[test]
fn test_negative_rationals() {
    assert_eval_to("-1/2", "-1/2");
    assert_eval_to("(- 1/2)", "-1/2");
    assert_eval_to("-3/4", "-3/4");
}

// ===== Rational Arithmetic =====

#[test]
fn test_rational_addition() {
    assert_eval_to("(+ 1/2 1/3)", "5/6");
    assert_eval_to("(+ 1/4 1/4)", "1/2");
    assert_eval_to("(+ 1/3 1/3 1/3)", "1");
    assert_eval_to("(+ 2/3 1/6)", "5/6");
    assert_eval_to("(+ 1/2 1/2)", "1");
}

#[test]
fn test_rational_subtraction() {
    assert_eval_to("(- 5/6 1/3)", "1/2");
    assert_eval_to("(- 3/4 1/4)", "1/2");
    assert_eval_to("(- 2/3 1/6)", "1/2");
    assert_eval_to("(- 1 1/2)", "1/2");
    assert_eval_to("(- 1/2 1)", "-1/2");
}

#[test]
fn test_rational_multiplication() {
    assert_eval_to("(* 1/2 1/3)", "1/6");
    assert_eval_to("(* 2/3 3/4)", "1/2");
    assert_eval_to("(* 3/4 4/5)", "3/5");
    assert_eval_to("(* 1/2 2)", "1");
    assert_eval_to("(* 2/3 3/2)", "1");
}

#[test]
fn test_rational_division() {
    assert_eval_to("(/ 1/2 1/3)", "3/2");
    assert_eval_to("(/ 2/3 4/5)", "5/6");
    assert_eval_to("(/ 1 2)", "1/2");
    assert_eval_to("(/ 1 3)", "1/3");
    assert_eval_to("(/ 3 4)", "3/4");
}

#[test]
fn test_rational_mixed_arithmetic() {
    assert_eval_to("(+ 1 1/2)", "3/2");
    assert_eval_to("(- 2 1/3)", "5/3");
    assert_eval_to("(* 3 2/3)", "2");
    assert_eval_to("(/ 4 3)", "4/3");
}

#[test]
fn test_rational_complex_expressions() {
    assert_eval_to("(+ (/ 1 2) (/ 1 3) (/ 1 6))", "1");
    assert_eval_to("(- (+ 1/2 1/3) 1/6)", "2/3");
    assert_eval_to("(* (+ 1/2 1/2) (- 2/3 1/6))", "1/2");
    assert_eval_to("(/ (+ 1/2 1/2) 2)", "1/2");
}

// ===== Rational Comparisons =====

#[test]
fn test_rational_equality() {
    assert_eval_to("(= 1/2 1/2)", "#t");
    assert_eval_to("(= 2/4 1/2)", "#t");
    assert_eval_to("(= 3/6 1/2)", "#t");
    assert_eval_to("(= 1/2 1/3)", "#f");
    assert_eval_to("(= 1/2 0.5)", "#t"); // Exact vs inexact comparison
}

#[test]
fn test_rational_less_than() {
    assert_eval_to("(< 1/3 1/2)", "#t");
    assert_eval_to("(< 1/4 1/3 1/2)", "#t");
    assert_eval_to("(< 1/2 1/3)", "#f");
    assert_eval_to("(< 2/5 1/2 3/5)", "#t");
}

#[test]
fn test_rational_greater_than() {
    assert_eval_to("(> 1/2 1/3)", "#t");
    assert_eval_to("(> 3/4 1/2 1/4)", "#t");
    assert_eval_to("(> 1/3 1/2)", "#f");
    assert_eval_to("(> 2/3 1/2 1/3)", "#t");
}

#[test]
fn test_rational_less_equal() {
    assert_eval_to("(<= 1/3 1/2)", "#t");
    assert_eval_to("(<= 1/2 1/2)", "#t");
    assert_eval_to("(<= 1/2 1/3)", "#f");
    assert_eval_to("(<= 1/4 1/3 1/2 1/2)", "#t");
}

#[test]
fn test_rational_greater_equal() {
    assert_eval_to("(>= 1/2 1/3)", "#t");
    assert_eval_to("(>= 1/2 1/2)", "#t");
    assert_eval_to("(>= 1/3 1/2)", "#f");
    assert_eval_to("(>= 2/3 1/2 1/2 1/3)", "#t");
}

// ===== Exact/Inexact Semantics =====

#[test]
fn test_rational_is_exact() {
    assert_eval_to("(exact? 1/2)", "#t");
    assert_eval_to("(exact? 3/4)", "#t");
    assert_eval_to("(exact? (/ 1 2))", "#t");
    assert_eval_to("(exact? (+ 1/2 1/3))", "#t");
}

#[test]
fn test_inexact_contagion_with_rationals() {
    assert_eval_to("(+ 1/2 0.5)", "1.0");
    assert_eval_to("(+ 0.5 1/2)", "1.0");
    assert_eval_to("(* 2/3 1.5)", "1.0");
    assert_eval_to("(- 1/2 0.25)", "0.25");
}

#[test]
fn test_rational_plus_inexact_is_inexact() {
    assert_eval_to("(inexact? (+ 1/2 0.5))", "#t");
    assert_eval_to("(inexact? (* 2/3 1.5))", "#t");
    assert_eval_to("(exact? (+ 1/2 0.5))", "#f");
}

#[test]
fn test_division_exactness() {
    // Exact division of integers should produce exact rationals
    assert_eval_to("(exact? (/ 1 2))", "#t");
    assert_eval_to("(exact? (/ 1 3))", "#t");
    assert_eval_to("(exact? (/ 22 7))", "#t");

    // Division involving inexact should produce inexact
    assert_eval_to("(inexact? (/ 1.0 2))", "#t");
    assert_eval_to("(inexact? (/ 1 2.0))", "#t");
}

// ===== Edge Cases =====

#[test]
fn test_rational_with_large_numbers() {
    assert_eval_to("(+ 1/1000 1/1000)", "1/500");
    assert_eval_to("(* 1/100 1/100)", "1/10000");
}

#[test]
fn test_rational_division_by_zero() {
    assert_eval_error("(/ 1 0)");
    assert_eval_error("(/ 1/2 0)");
}

#[test]
fn test_rational_very_large_denominators() {
    // Test with large denominators
    assert_eval_to("(+ 1/1000000 1/1000000)", "1/500000");
}

#[test]
fn test_rational_improper_fractions() {
    // Improper fractions (numerator > denominator)
    assert_eval_to("5/3", "5/3");
    assert_eval_to("7/4", "7/4");
    assert_eval_to("(+ 5/3 7/4)", "41/12");
}

#[test]
fn test_rational_reciprocal() {
    assert_eval_to("(/ 1/2)", "2");
    assert_eval_to("(/ 1/3)", "3");
    assert_eval_to("(/ 2/3)", "3/2");
    assert_eval_to("(/ 3/4)", "4/3");
}

#[test]
fn test_rational_zero() {
    assert_eval_to("(+ 1/2 0)", "1/2");
    assert_eval_to("(- 1/2 0)", "1/2");
    assert_eval_to("(* 1/2 0)", "0");
}

#[test]
fn test_rational_one() {
    assert_eval_to("(* 1/2 1)", "1/2");
    assert_eval_to("(/ 1/2 1)", "1/2");
    assert_eval_to("(+ 1/2 1/2)", "1");
}

#[test]
fn test_rational_negative_operations() {
    assert_eval_to("(+ -1/2 1/2)", "0");
    assert_eval_to("(* -1/2 2)", "-1");
    assert_eval_to("(/ -1 2)", "-1/2");
    assert_eval_to("(- 0 1/2)", "-1/2");
}

// ===== Rational Type Predicates =====

#[test]
fn test_rational_type_predicates() {
    assert_eval_to("(number? 1/2)", "#t");
    assert_eval_to("(integer? 1/2)", "#f");
    assert_eval_to("(integer? 2/1)", "#t"); // Simplified to integer
}

// ===== Special Cases from R7RS =====

#[test]
fn test_rational_associativity() {
    // (a + b) + c should equal a + (b + c)
    assert_eval_to("(= (+ (+ 1/2 1/3) 1/6) (+ 1/2 (+ 1/3 1/6)))", "#t");
}

#[test]
fn test_rational_commutativity() {
    // a + b should equal b + a
    assert_eval_to("(= (+ 1/2 1/3) (+ 1/3 1/2))", "#t");
    assert_eval_to("(= (* 2/3 3/4) (* 3/4 2/3))", "#t");
}

#[test]
fn test_rational_distributivity() {
    // a * (b + c) should equal (a * b) + (a * c)
    assert_eval_to("(= (* 1/2 (+ 1/3 1/4)) (+ (* 1/2 1/3) (* 1/2 1/4)))", "#t");
}

#[test]
fn test_rational_identity_elements() {
    // 0 is additive identity
    assert_eval_to("(= (+ 1/2 0) 1/2)", "#t");
    // 1 is multiplicative identity
    assert_eval_to("(= (* 1/2 1) 1/2)", "#t");
}

#[test]
fn test_rational_additive_inverse() {
    assert_eval_to("(= (+ 1/2 -1/2) 0)", "#t");
    assert_eval_to("(= (+ 2/3 (- 2/3)) 0)", "#t");
}

#[test]
fn test_rational_multiplicative_inverse() {
    assert_eval_to("(= (* 2/3 3/2) 1)", "#t");
    assert_eval_to("(= (* 1/2 (/ 1/2)) 1)", "#t");
}

// ===== Chibi-scheme Compatibility Tests =====

#[test]
fn test_rational_chibi_compatibility() {
    // These test cases are verified against chibi-scheme
    assert_eval_to("(+ 1/2 1/3)", "5/6");
    assert_eval_to("(- 5/6 1/3)", "1/2");
    assert_eval_to("(* 2/3 3/4)", "1/2");
    assert_eval_to("(/ 8/9 2/3)", "4/3");
    assert_eval_to("(= 2/4 1/2)", "#t");
    assert_eval_to("(< 1/3 1/2 2/3)", "#t");
}

#[test]
fn test_rational_chain_operations() {
    // Complex chained operations
    assert_eval_to("(+ 1/2 1/3 1/4 1/5)", "77/60");
    assert_eval_to("(* 1/2 1/3 1/4 1/5)", "1/120");
}

#[test]
fn test_rational_precision() {
    // Test that we maintain exact precision
    assert_eval_to("(= (+ 1/3 1/3 1/3) 1)", "#t");
    assert_eval_to("(= (+ 1/7 1/7 1/7 1/7 1/7 1/7 1/7) 1)", "#t");
}

#[test]
fn test_rational_with_negatives() {
    assert_eval_to("(+ 1/2 -1/3)", "1/6");
    assert_eval_to("(* -1/2 -1/3)", "1/6");
    assert_eval_to("(/ -1/2 -1/3)", "3/2");
}
