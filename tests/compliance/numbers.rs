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
    assert_eval_to("(/ 20 4 2)", "5/2"); // Exact division returns rationals
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
fn test_quotient() {
    assert_eval_to("(quotient 10 3)", "3");
    assert_eval_to("(quotient -10 3)", "-3");
}

#[test]
fn test_remainder() {
    assert_eval_to("(remainder 10 3)", "1");
    assert_eval_to("(remainder -10 3)", "-1");
}

#[test]
fn test_modulo() {
    assert_eval_to("(modulo 10 3)", "1");
    assert_eval_to("(modulo -10 3)", "2");
}

#[test]
fn test_abs() {
    assert_eval_to("(abs 5)", "5");
    assert_eval_to("(abs -5)", "5");
    assert_eval_to("(abs 0)", "0");
}

#[test]
fn test_max() {
    assert_eval_to("(max 1 2 3)", "3");
    assert_eval_to("(max 3 2 1)", "3");
}

#[test]
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
fn test_zero_predicate() {
    assert_eval_to("(zero? 0)", "#t");
    assert_eval_to("(zero? 1)", "#f");
}

#[test]
fn test_positive_predicate() {
    assert_eval_to("(positive? 5)", "#t");
    assert_eval_to("(positive? -5)", "#f");
    assert_eval_to("(positive? 0)", "#f");
}

#[test]
fn test_negative_predicate() {
    assert_eval_to("(negative? -5)", "#t");
    assert_eval_to("(negative? 5)", "#f");
    assert_eval_to("(negative? 0)", "#f");
}

#[test]
fn test_odd_predicate() {
    assert_eval_to("(odd? 3)", "#t");
    assert_eval_to("(odd? 4)", "#f");
}

#[test]
fn test_even_predicate() {
    assert_eval_to("(even? 4)", "#t");
    assert_eval_to("(even? 3)", "#f");
}

#[test]
fn test_exact_predicate() {
    // Exact numbers: Integer, BigInteger, Rational
    assert_eval_to("(exact? 42)", "#t");
    assert_eval_to("(exact? -17)", "#t");
    assert_eval_to("(exact? 0)", "#t");
    // BigInteger should be exact
    assert_eval_to("(exact? 9223372036854775808)", "#t");
    assert_eval_to("(exact? (- (+ 1 10000000000000000000000000000000000) 10000000000000000000000000000000000))", "#t"); // i64::MAX + 1 // i64::MAX + 1
    assert_eval_to("(exact? 10000000000000000000)", "#t");
}

#[test]
fn test_inexact_predicate() {
    // Inexact numbers: Real (floating point)
    assert_eval_to("(inexact? 3.14)", "#t");
    assert_eval_to("(inexact? 2.0)", "#t");
    assert_eval_to("(inexact? 0.0)", "#t");
    assert_eval_to("(inexact? -5.5)", "#t");
}

#[test]
fn test_exactness_distinction() {
    // Integer literals (no decimal point) are exact
    assert_eval_to("(exact? 123)", "#t");
    assert_eval_to("(inexact? 123)", "#f");

    // Decimal literals (with decimal point) are inexact
    assert_eval_to("(exact? 123.0)", "#f");
    assert_eval_to("(inexact? 123.0)", "#t");

    // This is the key R7RS behavior: syntactic exactness
    // 123 and 123.0 are mathematically equal but have different exactness
    // TODO: Requires mixed Integer/Real comparison in = operator
    // assert_eval_to("(= 123 123.0)", "#t"); // mathematically equal

    assert_eval_to("(exact? 123)", "#t"); // but different exactness
    assert_eval_to("(exact? 123.0)", "#f");
}

#[test]
fn test_inexact_contagion_addition() {
    // R7RS: Operations involving inexact numbers produce inexact results
    // exact + exact = exact
    assert_eval_to("(exact? (+ 1 2))", "#t");

    // inexact + inexact = inexact
    assert_eval_to("(inexact? (+ 1.0 2.0))", "#t");

    // exact + inexact = inexact (contagion)
    assert_eval_to("(inexact? (+ 1 2.0))", "#t");
    assert_eval_to("(inexact? (+ 1.0 2))", "#t");
}

#[test]
fn test_inexact_contagion_multiplication() {
    // exact * exact = exact
    assert_eval_to("(exact? (* 3 4))", "#t");

    // inexact * inexact = inexact
    assert_eval_to("(inexact? (* 3.0 4.0))", "#t");

    // exact * inexact = inexact (contagion)
    assert_eval_to("(inexact? (* 3 4.0))", "#t");
    assert_eval_to("(inexact? (* 3.0 4))", "#t");
}

#[test]
fn test_inexact_contagion_subtraction() {
    // exact - exact = exact
    assert_eval_to("(exact? (- 10 3))", "#t");

    // inexact - inexact = inexact
    assert_eval_to("(inexact? (- 10.0 3.0))", "#t");

    // exact - inexact = inexact (contagion)
    assert_eval_to("(inexact? (- 10 3.0))", "#t");
    assert_eval_to("(inexact? (- 10.0 3))", "#t");
}

#[test]
fn test_inexact_contagion_division() {
    // Division is special: even exact / exact can be inexact in some implementations
    // But exact / exact should produce exact rational if possible

    // inexact / inexact = inexact
    assert_eval_to("(inexact? (/ 10.0 3.0))", "#t");

    // exact / inexact = inexact (contagion)
    assert_eval_to("(inexact? (/ 10 3.0))", "#t");
    assert_eval_to("(inexact? (/ 10.0 3))", "#t");
}

#[test]
fn test_inexact_contagion_complex_expression() {
    // Once inexactness enters an expression, it propagates throughout

    // All exact
    assert_eval_to("(exact? (+ (* 2 3) (- 10 5)))", "#t");

    // One inexact contaminates the whole expression
    assert_eval_to("(inexact? (+ (* 2 3.0) (- 10 5)))", "#t");
    assert_eval_to("(inexact? (+ (* 2 3) (- 10.0 5)))", "#t");
}

#[test]
fn test_bigint_remains_exact() {
    // BigInteger arithmetic should remain exact
    assert_eval_to(
        "(exact? (+ 10000000000000000000 10000000000000000000))",
        "#t",
    );
    assert_eval_to("(exact? (* 10000000000000000000 2))", "#t");

    // Even i64 overflow promotion keeps exactness
    assert_eval_to("(exact? (+ 9223372036854775807 1))", "#t");
}

// Numeric tower tests - demonstrating overflow handling
#[test]
fn test_fibonacci_large() {
    // Fibonacci numbers grow exponentially
    // fib(100) = 354224848179261915075 (overflows i64::MAX = 9223372036854775807)
    // Using iterative version with let-values for O(n) performance
    assert_program_eval_to(
        r#"
        (define (fib n)
          (define (fib-iter a b count)
            (if (= count 0)
                a
                (let-values (((next-a next-b) (values b (+ a b))))
                  (fib-iter next-a next-b (- count 1)))))
          (fib-iter 0 1 n))

        (fib 100)
        "#,
        "354224848179261915075",
    );
}

#[test]
fn test_factorial_large() {
    // 25! = 15511210043330985984000000 (way beyond i64)
    assert_program_eval_to(
        r#"
        (define (factorial n)
          (if (= n 0)
              1
              (* n (factorial (- n 1)))))

        (factorial 25)
        "#,
        "15511210043330985984000000",
    );
}

#[test]
fn test_power_large() {
    // 2^100 = 1267650600228229401496703205376 (very large)
    assert_program_eval_to(
        r#"
        (define (power base exp)
          (if (= exp 0)
              1
              (* base (power base (- exp 1)))))

        (power 2 100)
        "#,
        "1267650600228229401496703205376",
    );
}

#[test]
fn test_arithmetic_overflow_add() {
    // i64::MAX = 9223372036854775807
    // Adding 1 should promote to BigInt
    assert_eval_to("(+ 9223372036854775807 1)", "9223372036854775808");
}

#[test]
fn test_arithmetic_overflow_multiply() {
    // 1000000000 * 10000000000 = 10000000000000000000 (overflows i64)
    assert_eval_to("(* 1000000000 10000000000)", "10000000000000000000");
}

// 6.2.6 Advanced algorithms using multiple values
// These demonstrate the power of values/call-with-values

#[test]
fn test_gcd_euclidean() {
    // Euclidean GCD algorithm using multiple values
    assert_program_eval_to(
        r#"
        (define (quotient-and-remainder a b)
          (values (quotient a b) (remainder a b)))

        (define (gcd a b)
          (if (= b 0)
              a
              (let-values (((q r) (quotient-and-remainder a b)))
                (gcd b r))))

        (gcd 48 18)
        "#,
        "6",
    );

    // More test cases
    assert_program_eval_to(
        r#"
        (define (quotient-and-remainder a b)
          (values (quotient a b) (remainder a b)))

        (define (gcd a b)
          (if (= b 0)
              a
              (let-values (((q r) (quotient-and-remainder a b)))
                (gcd b r))))

        (list (gcd 100 35) (gcd 54 24) (gcd 1071 462))
        "#,
        "(5 6 21)",
    );
}

#[test]
fn test_extended_gcd() {
    // Extended Euclidean algorithm - returns (gcd x y) where ax + by = gcd
    // This is a perfect demonstration of returning 3 values!
    assert_program_eval_to(
        r#"
        (define (extended-gcd a b)
          (if (= b 0)
              (values a 1 0)
              (let-values (((gcd x1 y1) (extended-gcd b (remainder a b))))
                (let ((q (quotient a b)))
                  (values gcd y1 (- x1 (* q y1)))))))

        ;; Test: extended-gcd(48, 18) should return (6, -1, 3)
        ;; Because: 48*(-1) + 18*3 = -48 + 54 = 6
        (call-with-values
          (lambda () (extended-gcd 48 18))
          (lambda (gcd x y)
            (list gcd x y)))
        "#,
        "(6 -1 3)",
    );
}

#[test]
fn test_quotient_remainder_relationship() {
    // Verify that quotient and remainder work correctly together
    // This uses the fundamental relationship: a = b*q + r
    assert_program_eval_to(
        r#"
        (let-values (((q r) (values (quotient 17 5) (remainder 17 5))))
          (list q r (+ (* 5 q) r)))
        "#,
        "(3 2 17)",
    );

    // Another example
    assert_program_eval_to(
        r#"
        (let-values (((q r) (values (quotient 100 7) (remainder 100 7))))
          (= 100 (+ (* 7 q) r)))
        "#,
        "#t",
    );
}
