//! R7RS Section 6.3 - Booleans and other predicates
//!
//! Tests based on chibi-scheme's r7rs-tests.scm
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

mod common;
use common::*;

// 6.3 Booleans
#[test]
fn test_boolean_values() {
    assert_eval_to("#t", "#t");
    assert_eval_to("#f", "#f");
}

#[test]
fn test_boolean_predicate() {
    assert_eval_to("(boolean? #t)", "#t");
    assert_eval_to("(boolean? #f)", "#t");
    assert_eval_to("(boolean? 0)", "#f");
    assert_eval_to("(boolean? '())", "#f");
}

#[test]
#[ignore] // TODO: Implement not
fn test_not() {
    assert_eval_to("(not #t)", "#f");
    assert_eval_to("(not #f)", "#t");
    assert_eval_to("(not 0)", "#f"); // Everything except #f is truthy
    assert_eval_to("(not '())", "#f");
}

#[test]
#[ignore] // TODO: Implement boolean=?
fn test_boolean_equal() {
    assert_eval_to("(boolean=? #t #t)", "#t");
    assert_eval_to("(boolean=? #f #f)", "#t");
    assert_eval_to("(boolean=? #t #f)", "#f");
}

// Equivalence predicates
#[test]
fn test_eq() {
    assert_eval_to("(eq? 'a 'a)", "#t");
    assert_eval_to("(eq? 'a 'b)", "#f");
    assert_eval_to("(eq? #t #t)", "#t");
    assert_eval_to("(eq? #f #f)", "#t");
}

#[test]
fn test_eqv() {
    assert_eval_to("(eqv? 'a 'a)", "#t");
    assert_eval_to("(eqv? 2 2)", "#t");
    assert_eval_to("(eqv? 2 3)", "#f");
    assert_eval_to("(eqv? '() '())", "#t");
}

#[test]
fn test_equal() {
    assert_eval_to("(equal? 'a 'a)", "#t");
    assert_eval_to("(equal? '(a b) '(a b))", "#t");
    assert_eval_to("(equal? '(a b) '(a c))", "#f");
}

// Type predicates
#[test]
fn test_symbol_predicate() {
    assert_eval_to("(symbol? 'a)", "#t");
    assert_eval_to("(symbol? 42)", "#f");
    assert_eval_to("(symbol? \"a\")", "#f");
}

#[test]
fn test_string_predicate() {
    assert_eval_to("(string? \"hello\")", "#t");
    assert_eval_to("(string? 'hello)", "#f");
    assert_eval_to("(string? 42)", "#f");
}

#[test]
#[ignore] // TODO: Implement char?
fn test_char_predicate() {
    assert_eval_to("(char? #\\a)", "#t");
    assert_eval_to("(char? \"a\")", "#f");
    assert_eval_to("(char? 97)", "#f");
}

#[test]
#[ignore] // TODO: Implement vector?
fn test_vector_predicate() {
    assert_eval_to("(vector? #(1 2 3))", "#t");
    assert_eval_to("(vector? '(1 2 3))", "#f");
}

#[test]
#[ignore] // TODO: Implement procedure?
fn test_procedure_predicate() {
    assert_eval_to("(procedure? +)", "#t");
    assert_eval_to("(procedure? (lambda (x) x))", "#t");
    assert_eval_to("(procedure? 42)", "#f");
}
