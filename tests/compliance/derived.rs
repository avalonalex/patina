//! R7RS Section 4.2 - Derived expression types
//!
//! Tests based on chibi-scheme's r7rs-tests.scm
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm
//!
//! Most of these are marked #[ignore] as they need to be implemented

use super::common::*;

// 4.2.1 Conditionals - Cond
#[test]
fn test_cond_simple() {
    assert_eval_to("(cond ((> 3 2) 'greater) ((< 3 2) 'less))", "greater");
}

#[test]
fn test_cond_with_else() {
    assert_eval_to(
        "(cond ((> 3 3) 'greater) ((< 3 3) 'less) (else 'equal))",
        "equal",
    );
}

#[test]
#[ignore] // TODO: Implement cond with =>
fn test_cond_with_arrow() {
    assert_eval_to("(cond ((assv 'b '((a 1) (b 2))) => cadr) (else #f))", "2");
}

// 4.2.1 Conditionals - Case
#[test]
#[ignore] // TODO: Implement case
fn test_case_simple() {
    assert_eval_to(
        "(case (* 2 3) ((2 3 5 7) 'prime) ((1 4 6 8 9) 'composite))",
        "composite",
    );
}

#[test]
#[ignore] // TODO: Implement case
fn test_case_with_else() {
    assert_eval_to(
        "(case (car '(c d)) ((a e i o u) 'vowel) ((w y) 'semivowel) (else 'consonant))",
        "consonant",
    );
}

// 4.2.2 Binding constructs - And/Or
#[test]
fn test_and_all_true() {
    assert_eval_to("(and (= 2 2) (> 2 1))", "#t");
}

#[test]
fn test_and_with_false() {
    assert_eval_to("(and (= 2 2) (< 2 1))", "#f");
}

#[test]
fn test_and_returns_last() {
    assert_eval_to("(and 1 2 'c '(f g))", "(f g)");
}

#[test]
fn test_and_empty() {
    assert_eval_to("(and)", "#t");
}

#[test]
fn test_or_first_true() {
    assert_eval_to("(or (= 2 2) (> 2 1))", "#t");
}

#[test]
fn test_or_all_false() {
    assert_eval_to("(or #f #f #f)", "#f");
}

#[test]
#[ignore] // TODO: Implement memq first
fn test_or_returns_first_true() {
    assert_eval_to("(or (memq 'b '(a b c)) (/ 3 0))", "(b c)");
}

// 4.2.2 Binding constructs - Let
#[test]
fn test_let_simple() {
    assert_eval_to("(let ((x 2) (y 3)) (* x y))", "6");
}

#[test]
fn test_let_scoping() {
    // Inner x shadows outer, but z uses outer x
    assert_eval_to(
        "(let ((x 2) (y 3)) (let ((x 7) (z (+ x y))) (* z x)))",
        "35",
    );
}

// 4.2.2 Binding constructs - Let*
#[test]
fn test_let_star_sequential() {
    // z can use x because let* is sequential
    assert_eval_to(
        "(let ((x 2) (y 3)) (let* ((x 7) (z (+ x y))) (* z x)))",
        "70",
    );
}

// 4.2.2 Binding constructs - Letrec
#[test]
fn test_letrec_recursive() {
    assert_eval_to(
        r#"(letrec ((even?
                   (lambda (n)
                     (if (= n 0)
                         #t
                         (odd? (- n 1)))))
                  (odd?
                   (lambda (n)
                     (if (= n 0)
                         #f
                         (even? (- n 1))))))
             (even? 88))"#,
        "#t",
    );
}

// 4.2.4 Iteration - Do
#[test]
#[ignore] // TODO: Implement do
fn test_do_simple() {
    assert_eval_to(
        r#"(do ((i 0 (+ i 1))
              (sum 0 (+ sum i)))
             ((> i 5) sum))"#,
        "15",
    );
}

// 4.2.6 Dynamic bindings - When/Unless
#[test]
#[ignore] // TODO: Implement when
fn test_when() {
    assert_eval_to("(when (> 3 2) 'yes)", "yes");
    assert_eval_to("(when (< 3 2) 'yes)", "#<unspecified>");
}

#[test]
#[ignore] // TODO: Implement unless
fn test_unless() {
    assert_eval_to("(unless (< 3 2) 'yes)", "yes");
    assert_eval_to("(unless (> 3 2) 'yes)", "#<unspecified>");
}
