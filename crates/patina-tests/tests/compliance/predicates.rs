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
fn test_char_predicate() {
    assert_eval_to("(char? #\\a)", "#t");
    assert_eval_to("(char? \"a\")", "#f");
    assert_eval_to("(char? 97)", "#f");
    assert_eval_to("(char? #\\世)", "#t");
}

#[test]
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

    // Core special forms error when referenced (not first-class values)
    assert_eval_error("(procedure? if)");
    assert_eval_error("(procedure? lambda)");
    assert_eval_error("(procedure? define)");
    assert_eval_error("(procedure? quote)");

    // Macros are not procedures
    // Note: let, let*, letrec, letrec*, and, or are now macros in bootstrap.scm
    // They can be referenced (unlike special forms), but are not procedures
    assert_eval_to("(procedure? let)", "#f");
    assert_eval_to("(procedure? let*)", "#f");
    assert_eval_to("(procedure? letrec)", "#f");
    assert_eval_to("(procedure? letrec*)", "#f");
    assert_eval_to("(procedure? and)", "#f");
    assert_eval_to("(procedure? or)", "#f");
    assert_eval_to("(procedure? cond)", "#f");
    assert_eval_to("(procedure? case)", "#f");
}

// Numeric type predicates (R7RS Section 6.2)
#[test]
fn test_complex_predicate() {
    // Complex numbers
    assert_eval_to("(complex? 3+4i)", "#t");
    assert_eval_to("(complex? 0+1i)", "#t");

    // Real numbers are also complex
    assert_eval_to("(complex? 3.14)", "#t");
    assert_eval_to("(complex? 22/7)", "#t");
    assert_eval_to("(complex? 42)", "#t");

    // Non-numbers
    assert_eval_to("(complex? \"42\")", "#f");
    assert_eval_to("(complex? #t)", "#f");
}

#[test]
fn test_real_predicate() {
    // Real numbers
    assert_eval_to("(real? 3.14)", "#t");
    assert_eval_to("(real? 22/7)", "#t");
    assert_eval_to("(real? 42)", "#t");
    assert_eval_to("(real? -5)", "#t");

    // Complex numbers with non-zero imaginary part
    assert_eval_to("(real? 3+4i)", "#f");

    // Non-numbers
    assert_eval_to("(real? \"3.14\")", "#f");
    assert_eval_to("(real? #f)", "#f");
}

#[test]
fn test_rational_predicate() {
    // Rationals
    assert_eval_to("(rational? 22/7)", "#t");
    assert_eval_to("(rational? 3/4)", "#t");

    // Integers are also rational
    assert_eval_to("(rational? 42)", "#t");
    assert_eval_to("(rational? -5)", "#t");

    // R7RS: finite inexact numbers are also rational
    // (every finite float can be expressed as a ratio)
    assert_eval_to("(rational? 3.14)", "#t");

    // Complex numbers
    assert_eval_to("(rational? 3+4i)", "#f");

    // Non-numbers
    assert_eval_to("(rational? \"22/7\")", "#f");
}

#[test]
fn test_integer_predicate() {
    // Integers
    assert_eval_to("(integer? 42)", "#t");
    assert_eval_to("(integer? -5)", "#t");
    assert_eval_to("(integer? 0)", "#t");

    // Rationals that are not integers
    assert_eval_to("(integer? 22/7)", "#f");
    assert_eval_to("(integer? 3/4)", "#f");

    // Reals
    assert_eval_to("(integer? 3.14)", "#f");
    // R7RS: inexact integers like 3.0 are also integers
    assert_eval_to("(integer? 3.0)", "#t");

    // Complex
    assert_eval_to("(integer? 3+4i)", "#f");

    // Non-numbers
    assert_eval_to("(integer? \"42\")", "#f");
}

#[test]
fn test_number_predicate() {
    // All numeric types
    assert_eval_to("(number? 42)", "#t");
    assert_eval_to("(number? 22/7)", "#t");
    assert_eval_to("(number? 3.14)", "#t");
    assert_eval_to("(number? 3+4i)", "#t");

    // Non-numbers
    assert_eval_to("(number? \"42\")", "#f");
    assert_eval_to("(number? #t)", "#f");
    assert_eval_to("(number? 'symbol)", "#f");
}

// ===== R7RS Inexact Integer Tests =====
// These tests verify R7RS-compliant handling of inexact integers

#[test]
fn test_inexact_integer_predicates() {
    // R7RS 6.2.6: integer? returns #t for inexact integers
    assert_eval_to("(integer? 4.0)", "#t");
    assert_eval_to("(integer? -0.0)", "#t");
    assert_eval_to("(integer? 1000000.0)", "#t");

    // Non-integer inexact numbers
    assert_eval_to("(integer? 4.5)", "#f");
    assert_eval_to("(integer? 0.1)", "#f");

    // R7RS: rational? returns #t for all finite reals
    assert_eval_to("(rational? 4.0)", "#t");
    assert_eval_to("(rational? 3.14159)", "#t");
    assert_eval_to("(rational? 0.1)", "#t");
}

#[test]
fn test_modulo_r7rs_semantics() {
    // R7RS modulo: result has the same sign as the divisor
    // This is different from Euclidean remainder (rem_euclid)
    assert_eval_to("(modulo 13 4)", "1");
    assert_eval_to("(modulo 13 -4)", "-3");
    assert_eval_to("(modulo -13 4)", "3");
    assert_eval_to("(modulo -13 -4)", "-1");
}

#[test]
fn test_inexact_integer_operations() {
    // R7RS 6.2.6: quotient, remainder, modulo work with inexact integers
    // Quotient tests
    assert_eval_to("(quotient 13 4.0)", "3.0");
    assert_eval_to("(quotient 13.0 4)", "3.0");
    assert_eval_to("(quotient 13.0 4.0)", "3.0");

    // Remainder tests
    assert_eval_to("(remainder -13 -4.0)", "-1.0");
    assert_eval_to("(remainder -13.0 -4)", "-1.0");
    assert_eval_to("(remainder -13.0 -4.0)", "-1.0");

    // Modulo tests (result has same sign as divisor)
    assert_eval_to("(modulo -13 4.0)", "3.0");
    assert_eval_to("(modulo -13.0 4)", "3.0");
    assert_eval_to("(modulo -13.0 4.0)", "3.0");
    assert_eval_to("(modulo -13 -4.0)", "-1.0");
    assert_eval_to("(modulo -13.0 -4)", "-1.0");
    assert_eval_to("(modulo -13.0 -4.0)", "-1.0");
    assert_eval_to("(modulo 13.0 -4)", "-3.0");
    assert_eval_to("(modulo 13 -4.0)", "-3.0");

    // R7RS 6.2.6: gcd and lcm work with inexact integers
    assert_eval_to("(gcd 32.0 -36)", "4.0");
    assert_eval_to("(gcd 32 -36.0)", "4.0");
    assert_eval_to("(gcd 32.0 -36.0)", "4.0");

    assert_eval_to("(lcm 32.0 -36)", "288.0");
    assert_eval_to("(lcm 32 -36.0)", "288.0");
    assert_eval_to("(lcm 32.0 -36.0)", "288.0");
}

#[test]
fn test_numerator_denominator_inexact() {
    // R7RS 6.2.6: numerator/denominator work with inexact rationals
    // Results are inexact when input is inexact
    assert_eval_to("(numerator 5.0)", "5.0");
    assert_eval_to("(denominator 5.0)", "1.0");

    // 5.5 = 11/2
    assert_eval_to("(numerator 5.5)", "11.0");
    assert_eval_to("(denominator 5.5)", "2.0");

    // Verify (/ (numerator x) (denominator x)) = x
    assert_eval_to("(/ (numerator 3.4) (denominator 3.4))", "3.4");
    assert_eval_to("(/ (numerator 0.5) (denominator 0.5))", "0.5");
}

// ===== R7RS Complex Number with Zero Imaginary Part Tests =====
// These tests verify that complex numbers with zero imaginary part
// satisfy the appropriate numeric predicates

#[test]
fn test_complex_zero_imaginary_predicates() {
    // R7RS: Complex with zero imaginary part is real
    assert_eval_to("(real? 3+0i)", "#t");
    assert_eval_to("(real? -2.5+0i)", "#t");
    assert_eval_to("(real? 3+4i)", "#f");

    // R7RS: Complex with zero imaginary and integer real is an integer
    assert_eval_to("(integer? 3+0i)", "#t");
    assert_eval_to("(integer? 4.0+0i)", "#t");
    assert_eval_to("(integer? 3.5+0i)", "#f");
    assert_eval_to("(integer? 3+4i)", "#f");

    // R7RS: Complex with zero imaginary and finite real is rational
    assert_eval_to("(rational? 3+0i)", "#t");
    assert_eval_to("(rational? 3.14+0i)", "#t");
    assert_eval_to("(rational? 3+4i)", "#f");
}

#[test]
fn test_odd_even_negative() {
    // R7RS: odd? and even? should work with negative numbers
    assert_eval_to("(odd? -1)", "#t");
    assert_eval_to("(odd? -3)", "#t");
    assert_eval_to("(odd? -2)", "#f");
    assert_eval_to("(even? -2)", "#t");
    assert_eval_to("(even? -4)", "#t");
    assert_eval_to("(even? -1)", "#f");
}

#[test]
fn test_expt_inexact_contagion() {
    // R7RS: expt should return inexact when either operand is inexact
    assert_eval_to("(expt 0 0)", "1");
    assert_eval_to("(expt 0.0 0)", "1.0");
    assert_eval_to("(expt 0 0.0)", "1.0");
    assert_eval_to("(expt 0.0 0.0)", "1.0");
}
