//! R7RS Section 4.1 - Primitive expression types
//!
//! Tests based on chibi-scheme's r7rs-tests.scm
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

use super::common::*;

// 4.1.1 Variable references
#[test]
fn test_variable_reference() {
    assert_program_eval_to(
        r#"
        (define x 28)
        x
        "#,
        "28",
    );
}

// 4.1.2 Literal expressions - Quote
#[test]
fn test_quote_symbol() {
    assert_eval_to("(quote a)", "a");
    assert_eval_to("'a", "a");
}

#[test]
fn test_quote_list() {
    assert_eval_to("'(+ 1 2)", "(+ 1 2)");
    assert_eval_to("(quote (+ 1 2))", "(+ 1 2)");
}

#[test]
fn test_quote_nested() {
    assert_eval_to("'(quote a)", "(quote a)");
    assert_eval_to("''a", "(quote a)");
}

#[test]
fn test_quote_literals() {
    assert_eval_to(r#""abc""#, r#""abc""#);
    assert_eval_to("145932", "145932");
    assert_eval_to("#t", "#t");
    assert_eval_to("#f", "#f");
}

// 4.1.4 Procedures - Lambda
#[test]
fn test_simple_lambda() {
    assert_eval_to("((lambda (x) (+ x x)) 4)", "8");
}

#[test]
fn test_lambda_multiple_args() {
    assert_program_eval_to(
        r#"
        (define reverse-subtract
          (lambda (x y) (- y x)))
        (reverse-subtract 7 10)
        "#,
        "3",
    );
}

#[test]
#[ignore] // TODO: Requires let binding form
fn test_lambda_closure() {
    assert_program_eval_to(
        r#"
        (define add4
          (let ((x 4))
            (lambda (y) (+ x y))))
        (add4 6)
        "#,
        "10",
    );
}

#[test]
fn test_lambda_variadic() {
    assert_eval_to("((lambda x x) 3 4 5 6)", "(3 4 5 6)");
}

#[test]
fn test_lambda_variadic_with_fixed() {
    assert_eval_to("((lambda (x y . z) z) 3 4 5 6)", "(5 6)");
}

// 4.1.5 Conditionals - If
#[test]
fn test_if_true() {
    assert_eval_to("(if (> 3 2) 'yes 'no)", "yes");
    assert_eval_to("(if #t 'yes 'no)", "yes");
}

#[test]
fn test_if_false() {
    assert_eval_to("(if (> 2 3) 'yes 'no)", "no");
    assert_eval_to("(if #f 'yes 'no)", "no");
}

#[test]
fn test_if_consequent_evaluated() {
    assert_eval_to("(if (> 3 2) (- 3 2) (+ 3 2))", "1");
}

#[test]
fn test_if_procedure_selection() {
    assert_eval_to("((if #f + *) 3 4)", "12");
}

// 4.1.6 Assignments - Define and Set!
#[test]
fn test_define_variable() {
    assert_program_eval_to(
        r#"
        (define x 10)
        x
        "#,
        "10",
    );
}

#[test]
fn test_define_multiple() {
    assert_program_eval_to(
        r#"
        (define x 2)
        (define y 3)
        (+ x y)
        "#,
        "5",
    );
}

#[test]
fn test_set_bang() {
    assert_program_eval_to(
        r#"
        (define x 10)
        (set! x 20)
        x
        "#,
        "20",
    );
}

#[test]
#[ignore] // TODO: Should error on undefined variable
fn test_set_bang_undefined() {
    assert_eval_error("(set! undefined-var 42)");
}

// Begin
#[test]
fn test_begin() {
    assert_eval_to("(begin (define a 5) (define b 10) (+ a b))", "15");
}

#[test]
fn test_begin_returns_last() {
    assert_eval_to("(begin 1 2 3)", "3");
}
