//! R7RS Section 6.2 - Numbers
//!
//! Tests based on chibi-scheme's r7rs-tests.scm
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

use super::common::*;

// 6.2.6 Numerical operations - Arithmetic
#[test]
fn test_addition() {
    assert_eval_to("(+ 3 4)", "7");
    assert_eval_to("(+ 1 2 3)", "6");
    assert_eval_to("(+)", "0");
}

#[test]
fn test_subtraction() {
    assert_eval_to("(- 10 3)", "7");
    assert_eval_to("(- 10 3 2)", "5");
    assert_eval_to("(- 5)", "-5");
}

#[test]
fn test_multiplication() {
    assert_eval_to("(* 2 3)", "6");
    assert_eval_to("(* 2 3 4)", "24");
    assert_eval_to("(*)", "1");
}

#[test]
fn test_division() {
    assert_eval_to("(/ 20 4)", "5");
    assert_eval_to("(/ 20 4 2)", "2.5");
}

#[test]
#[ignore] // TODO: Division by zero should error
fn test_division_by_zero() {
    assert_eval_error("(/ 1 0)");
}

// 6.2.6 Numerical operations - Comparisons
#[test]
fn test_equal() {
    assert_eval_to("(= 5 5)", "#t");
    assert_eval_to("(= 5 6)", "#f");
    assert_eval_to("(= 5 5 5)", "#t");
    assert_eval_to("(= 5 5 6)", "#f");
}

#[test]
fn test_less_than() {
    assert_eval_to("(< 1 2)", "#t");
    assert_eval_to("(< 2 1)", "#f");
    assert_eval_to("(< 1 2 3)", "#t");
    assert_eval_to("(< 1 2 2)", "#f");
}

#[test]
fn test_greater_than() {
    assert_eval_to("(> 2 1)", "#t");
    assert_eval_to("(> 1 2)", "#f");
    assert_eval_to("(> 3 2 1)", "#t");
}

#[test]
fn test_less_equal() {
    assert_eval_to("(<= 1 2)", "#t");
    assert_eval_to("(<= 2 2)", "#t");
    assert_eval_to("(<= 2 1)", "#f");
}

#[test]
fn test_greater_equal() {
    assert_eval_to("(>= 2 1)", "#t");
    assert_eval_to("(>= 2 2)", "#t");
    assert_eval_to("(>= 1 2)", "#f");
}

// 6.2.6 Numerical operations - Additional functions
#[test]
#[ignore] // TODO: Implement quotient
fn test_quotient() {
    assert_eval_to("(quotient 10 3)", "3");
    assert_eval_to("(quotient -10 3)", "-3");
}

#[test]
#[ignore] // TODO: Implement remainder
fn test_remainder() {
    assert_eval_to("(remainder 10 3)", "1");
    assert_eval_to("(remainder -10 3)", "-1");
}

#[test]
#[ignore] // TODO: Implement modulo
fn test_modulo() {
    assert_eval_to("(modulo 10 3)", "1");
    assert_eval_to("(modulo -10 3)", "2");
}

#[test]
#[ignore] // TODO: Implement abs
fn test_abs() {
    assert_eval_to("(abs 5)", "5");
    assert_eval_to("(abs -5)", "5");
    assert_eval_to("(abs 0)", "0");
}

#[test]
#[ignore] // TODO: Implement max
fn test_max() {
    assert_eval_to("(max 1 2 3)", "3");
    assert_eval_to("(max 3 2 1)", "3");
}

#[test]
#[ignore] // TODO: Implement min
fn test_min() {
    assert_eval_to("(min 1 2 3)", "1");
    assert_eval_to("(min 3 2 1)", "1");
}

// 6.2.6 Predicates
#[test]
fn test_number_predicate() {
    assert_eval_to("(number? 42)", "#t");
    assert_eval_to("(number? 'a)", "#f");
}

#[test]
fn test_integer_predicate() {
    assert_eval_to("(integer? 42)", "#t");
    assert_eval_to("(integer? 3.14)", "#f");
}

#[test]
#[ignore] // TODO: Implement zero?
fn test_zero_predicate() {
    assert_eval_to("(zero? 0)", "#t");
    assert_eval_to("(zero? 1)", "#f");
}

#[test]
#[ignore] // TODO: Implement positive?
fn test_positive_predicate() {
    assert_eval_to("(positive? 5)", "#t");
    assert_eval_to("(positive? -5)", "#f");
    assert_eval_to("(positive? 0)", "#f");
}

#[test]
#[ignore] // TODO: Implement negative?
fn test_negative_predicate() {
    assert_eval_to("(negative? -5)", "#t");
    assert_eval_to("(negative? 5)", "#f");
    assert_eval_to("(negative? 0)", "#f");
}

#[test]
#[ignore] // TODO: Implement odd?
fn test_odd_predicate() {
    assert_eval_to("(odd? 3)", "#t");
    assert_eval_to("(odd? 4)", "#f");
}

#[test]
#[ignore] // TODO: Implement even?
fn test_even_predicate() {
    assert_eval_to("(even? 4)", "#t");
    assert_eval_to("(even? 3)", "#f");
}
