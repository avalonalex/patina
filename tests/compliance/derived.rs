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
fn test_cond_with_arrow() {
    assert_eval_to("(cond ((assv 'b '((a 1) (b 2))) => cadr) (else #f))", "2");
}

#[test]
fn test_cond_with_arrow_false() {
    // When test is #f, should try next clause
    assert_eval_to(
        "(cond ((assv 'z '((a 1) (b 2))) => cadr) (else 'not-found))",
        "not-found",
    );
}

#[test]
fn test_cond_arrow_with_lambda() {
    // Can use lambda with =>
    assert_eval_to(
        "(cond ((memq 'b '(a b c)) => (lambda (x) (list 'found x))) (else #f))",
        "(found (b c))",
    );
}

// 4.2.1 Conditionals - Case
#[test]
fn test_case_simple() {
    assert_eval_to(
        "(case (* 2 3) ((2 3 5 7) 'prime) ((1 4 6 8 9) 'composite))",
        "composite",
    );
}

#[test]
fn test_case_with_else() {
    assert_eval_to(
        "(case (car '(c d)) ((a e i o u) 'vowel) ((w y) 'semivowel) (else 'consonant))",
        "consonant",
    );
}

#[test]
fn test_case_with_arrow() {
    // (else => proc) - passes the key to proc
    assert_eval_to(
        "(case (car '(c d)) ((a e i o u) 'vowel) ((w y) 'semivowel) (else => (lambda (x) x)))",
        "c",
    );
}

#[test]
fn test_case_no_match() {
    // No clause matches and no else - returns unspecified
    assert_program_eval_to(
        r#"(define result (case 10 ((1 2 3) 'small) ((20 30) 'big)))
           result"#,
        "#<unspecified>",
    );
}

#[test]
fn test_case_single_datum() {
    assert_eval_to("(case 'a ((a) 'matched))", "matched");
}

#[test]
fn test_case_with_multiple_exprs() {
    // Case clause can have multiple expressions
    assert_program_eval_to(
        r#"(define x 0)
           (case 1 ((1) (set! x 10) (set! x (+ x 5)) x))"#,
        "15",
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

// 4.2.2 Binding Constructs - let-values and let*-values
#[test]
fn test_let_values_simple() {
    assert_eval_to("(let-values (((a b) (values 1 2))) (+ a b))", "3");
}

#[test]
fn test_let_values_multiple_bindings() {
    assert_eval_to(
        "(let-values (((a b) (values 1 2)) ((c d) (values 3 4))) (+ a b c d))",
        "10",
    );
}

#[test]
fn test_let_star_values_sequential() {
    // Example from chibi tests - c and d use a and b from previous binding
    assert_program_eval_to(
        r#"
        (let ((x 'x) (y 'y))
          (let*-values (((a b) (values x y))
                        ((x y) (values a b)))
            (list a b x y)))
        "#,
        "(x y x y)",
    );
}

#[test]
fn test_let_star_values_dependency() {
    assert_eval_to(
        "(let*-values (((a b) (values 1 2)) ((c d) (values a b))) (list a b c d))",
        "(1 2 1 2)",
    );
}

#[test]
fn test_let_values_empty() {
    // Empty bindings
    assert_eval_to("(let-values () 42)", "42");
}

// 4.3 Macros (define-syntax, syntax-rules)

#[test]
fn test_simple_macro() {
    assert_program_eval_to(
        r#"
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))
        (when #t 42)
        "#,
        "42",
    );
}

#[test]
fn test_simple_macro_false() {
    assert_program_eval_to(
        r#"
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))
        (when #f 42)
        "#,
        "#<unspecified>",
    );
}

#[test]
fn test_nested_macros() {
    // This is the key test - verifies nested macro calls work
    // The inner 'my-unless' macro should NOT be renamed by hygiene
    assert_program_eval_to(
        r#"
        (define-syntax my-when
          (syntax-rules ()
            ((my-when test body ...)
             (if test (begin body ...)))))

        (define-syntax my-unless
          (syntax-rules ()
            ((my-unless test body ...)
             (if (not test) (begin body ...)))))

        (my-when #t
          (my-unless #f
            42))
        "#,
        "42",
    );
}

#[test]
fn test_nested_macros_complex() {
    // More complex nesting with multiple macro calls
    assert_program_eval_to(
        r#"
        (define-syntax my-when
          (syntax-rules ()
            ((my-when test body ...)
             (if test (begin body ...)))))

        (define-syntax my-unless
          (syntax-rules ()
            ((my-unless test body ...)
             (if (not test) (begin body ...)))))

        (define result
          (my-when #t
            (my-unless #f
              (my-when #t
                (my-unless #f
                  123)))))
        result
        "#,
        "123",
    );
}

#[test]
fn test_macro_hygiene_prevents_capture() {
    // Verify that hygiene prevents variable capture
    // The macro's 'temp' should be renamed, but user variables x, y, temp should not be
    assert_program_eval_to(
        r#"
        (define-syntax swap!
          (syntax-rules ()
            ((swap! a b)
             (let ((temp a))
               (set! a b)
               (set! b temp)))))

        (define temp 999)
        (define x 1)
        (define y 2)
        (swap! x y)
        (list x y temp)
        "#,
        "(2 1 999)",  // temp should still be 999, not captured
    );
}
