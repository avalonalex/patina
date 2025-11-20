mod common;
use common::*;

#[test]
fn test_vertical_bar_basic() {
    assert_program_eval_to("(define |hello world| 42) |hello world|", "42");
}

#[test]
fn test_vertical_bar_with_special_chars() {
    assert_program_eval_to("(define |foo(bar)baz| 100) |foo(bar)baz|", "100");
}

#[test]
fn test_vertical_bar_with_hex_escape() {
    // |H\x65;llo| should be equivalent to Hello
    assert_program_eval_to("(define |H\\x65;llo| 123) Hello", "123");
    assert_program_eval_to("(define Hello 456) |H\\x65;llo|", "456");
}

#[test]
fn test_vertical_bar_empty() {
    assert_program_eval_to("(define || \"empty\") ||", "\"empty\"");
}

#[test]
fn test_vertical_bar_as_function_name() {
    assert_program_eval_to(
        "(define (|add two numbers| a b) (+ a b)) (|add two numbers| 10 20)",
        "30",
    );
}

#[test]
fn test_vertical_bar_unicode() {
    // |\x3BB;| is λ (lambda)
    assert_program_eval_to(
        "(define |\\x3BB;| \"lambda symbol\") |\\x3BB;|",
        "\"lambda symbol\"",
    );
}

#[test]
fn test_vertical_bar_with_tabs() {
    // |\t\t| should contain two tabs
    assert_program_eval_to("(define |\\t\\t| 999) |\\t\\t|", "999");
}

#[test]
fn test_vertical_bar_with_escaped_pipe() {
    // |foo\|bar| contains a literal pipe character
    assert_program_eval_to("(define |foo\\|bar| 777) |foo\\|bar|", "777");
}

#[test]
fn test_vertical_bar_in_lambda() {
    assert_eval_to("((lambda (|x y|) (+ |x y| 5)) 10)", "15");
}

#[test]
fn test_vertical_bar_with_quote() {
    // When displaying a symbol with special characters (like spaces),
    // it should be displayed with vertical bars so it can be read back correctly
    assert_eval_to("'|hello world|", "|hello world|");
}

#[test]
fn test_vertical_bar_display_edge_cases() {
    // R7RS test suite edge cases - symbols that need vertical bars
    assert_eval_to("'|.|", "|.|"); // single dot
    assert_eval_to("'|,a|", "|,a|"); // starts with comma
    assert_eval_to(r#"'|"|"#, r#"|"|"#); // contains quote
    assert_eval_to("'|2|", "|2|"); // starts with digit
    assert_eval_to("'|+3|", "|+3|"); // looks like number
    assert_eval_to("'|-.4|", "|-.4|"); // looks like float
    assert_eval_to("'|+i|", "|+i|"); // imaginary number
    assert_eval_to("'|-i|", "|-i|"); // imaginary number
    assert_eval_to("'|+inf.0|", "|+inf.0|"); // special float
    assert_eval_to("'|-inf.0|", "|-inf.0|"); // special float
    assert_eval_to("'|+nan.0|", "|+nan.0|"); // special float
    assert_eval_to("'|+NaN.0|", "|+NaN.0|"); // case insensitive
    assert_eval_to("'|+NaN.0abc|", "|+NaN.0abc|"); // starts like number
    assert_eval_to("'|123abc|", "|123abc|"); // starts like number
}

#[test]
fn test_vertical_bar_display_no_bars_needed() {
    // Symbols that don't need vertical bars
    assert_eval_to("'|test|", "test"); // normal identifier
    assert_eval_to("'|hello|", "hello"); // normal identifier
    assert_eval_to("'|a-b-c|", "a-b-c"); // hyphens are fine
}
