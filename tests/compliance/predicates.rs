//! R7RS Section 6.3 - Booleans and other predicates
//!
//! Tests based on chibi-scheme's r7rs-tests.scm
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

use super::common::*;

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
fn test_not() {
    assert_eval_to("(not #t)", "#f");
    assert_eval_to("(not #f)", "#t");
    assert_eval_to("(not 0)", "#f"); // Everything except #f is truthy
    assert_eval_to("(not '())", "#f");
}

#[test]
fn test_boolean_equal() {
    assert_eval_to("(boolean=? #t #t)", "#t");
    assert_eval_to("(boolean=? #f #f)", "#t");
    assert_eval_to("(boolean=? #t #f)", "#f");
    // Variadic tests
    assert_eval_to("(boolean=? #t #t #t)", "#t");
    assert_eval_to("(boolean=? #f #f #f)", "#t");
    assert_eval_to("(boolean=? #t #t #f)", "#f");
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
fn test_procedure_predicate() {
    // Basic cases - primitives and lambdas
    assert_eval_to("(procedure? +)", "#t");
    assert_eval_to("(procedure? cons)", "#t");
    assert_eval_to("(procedure? (lambda (x) x))", "#t");
    assert_eval_to("(procedure? (lambda () 3))", "#t");

    // Non-procedures
    assert_eval_to("(procedure? 42)", "#f");
    assert_eval_to("(procedure? #t)", "#f");
    assert_eval_to("(procedure? \"hello\")", "#f");
    assert_eval_to("(procedure? 'symbol)", "#f");
    assert_eval_to("(procedure? '())", "#f");
    assert_eval_to("(procedure? '(1 2 3))", "#f");
    assert_eval_to("(procedure? (cons 1 2))", "#f");

    // Evaluated results
    assert_eval_to("(procedure? ((lambda () 3)))", "#f");
    assert_eval_to("(procedure? ((lambda () +)))", "#t");

    // Self-reference
    assert_eval_to("(procedure? procedure?)", "#t");

    // Higher-order: lambda returning procedure
    assert_eval_to("(procedure? (lambda () +))", "#t");

    // Closures and named procedures
    assert_program_eval_to(
        "(define (make-adder n) (lambda (x) (+ x n)))
         (procedure? make-adder)",
        "#t",
    );
    assert_program_eval_to(
        "(define (make-adder n) (lambda (x) (+ x n)))
         (procedure? (make-adder 5))",
        "#t",
    );
    assert_program_eval_to(
        "(define (factorial n) (if (= n 0) 1 (* n (factorial (- n 1)))))
         (procedure? factorial)",
        "#t",
    );

    // Curried functions
    assert_program_eval_to(
        "(define (curry f) (lambda (x) (lambda (y) (f x y))))
         (procedure? (curry +))",
        "#t",
    );
    assert_program_eval_to(
        "(define (curry f) (lambda (x) (lambda (y) (f x y))))
         (procedure? ((curry +) 5))",
        "#t",
    );

    // Results from apply
    assert_eval_to("(procedure? (apply + '()))", "#f"); // Returns 0
    assert_eval_to("(procedure? (apply (lambda () +) '()))", "#t"); // Returns +

    // Map results
    assert_eval_to("(procedure? (map (lambda (x) x) '(1 2)))", "#f"); // Returns list
    assert_eval_to("(procedure? (car (map (lambda (x) +) '(1))))", "#t"); // List of procedures

    // Special forms are not procedures (they error when referenced)
    assert_eval_error("(procedure? cond)");
    assert_eval_error("(procedure? if)");
    assert_eval_error("(procedure? lambda)");
    assert_eval_error("(procedure? case)");
    assert_eval_error("(procedure? let)");
    assert_eval_error("(procedure? letrec)");
    assert_eval_error("(procedure? define)");
    assert_eval_error("(procedure? quote)");
}
