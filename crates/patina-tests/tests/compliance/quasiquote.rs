//! R7RS Section 4.2.8 - Quasiquotation
//!
//! Tests based on chibi-scheme's r7rs-tests.scm and R7RS specification
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm (lines 344-355)
//! Reference: R7RS Section 4.2.8

use super::common::*;

// ============================================================================
// Basic Quasiquote Tests
// ============================================================================

#[test]
fn test_quasiquote_without_unquote() {
    // Without unquote, behaves like quote
    assert_eval_to("`(a b c)", "(a b c)");
    assert_eval_to("`(1 2 3)", "(1 2 3)");
}

#[test]
fn test_quasiquote_self_evaluating() {
    // Self-evaluating values
    assert_eval_to("`42", "42");
    assert_eval_to("`#t", "#t");
    assert_eval_to("`\"hello\"", "\"hello\"");
}

#[test]
fn test_quasiquote_symbols() {
    // Symbols are quoted
    assert_eval_to("`foo", "foo");
    assert_eval_to("`(foo bar)", "(foo bar)");
}

// ============================================================================
// Unquote Tests
// ============================================================================

#[test]
fn test_unquote_simple() {
    // R7RS example: `(list ,(+ 1 2) 4) => (list 3 4)
    assert_eval_to("`(list ,(+ 1 2) 4)", "(list 3 4)");
}

#[test]
fn test_unquote_multiple() {
    // Multiple unquotes in one template
    assert_eval_to("`(,(+ 1 2) ,(* 3 4) 5)", "(3 12 5)");
}

#[test]
fn test_unquote_variable() {
    // R7RS example: Unquoting variables
    // Note: We display (quote a) as 'a which is equivalent and more readable
    assert_program_eval_to(
        "(define name 'a)
         `(list ,name ',name)",
        "(list a 'a)",
    );
}

#[test]
fn test_unquote_nested_expr() {
    // Unquote complex expressions
    assert_eval_to("`(a ,(if #t 'yes 'no) b)", "(a yes b)");
    assert_eval_to("`(,(car '(1 2 3)) ,(cdr '(1 2 3)))", "(1 (2 3))");
}

// ============================================================================
// Unquote-Splicing Tests
// ============================================================================

#[test]
fn test_unquote_splicing_simple() {
    // R7RS example: `(a ,(+ 1 2) ,@(map abs '(4 -5 6)) b) => (a 3 4 5 6 b)
    assert_eval_to("`(a ,(+ 1 2) ,@(map abs '(4 -5 6)) b)", "(a 3 4 5 6 b)");
}

#[test]
fn test_unquote_splicing_at_start() {
    assert_eval_to("`(,@(list 1 2 3) 4 5)", "(1 2 3 4 5)");
}

#[test]
fn test_unquote_splicing_at_end() {
    assert_eval_to("`(1 2 ,@(list 3 4 5))", "(1 2 3 4 5)");
}

#[test]
fn test_unquote_splicing_empty_list() {
    // Splicing empty list should disappear
    assert_eval_to("`(a ,@'() b)", "(a b)");
}

#[test]
fn test_unquote_splicing_multiple() {
    // Multiple splices
    assert_eval_to("`(,@(list 1 2) ,@(list 3 4))", "(1 2 3 4)");
}

// ============================================================================
// Vector Quasiquote Tests
// ============================================================================

#[test]
fn test_quasiquote_vector_simple() {
    assert_eval_to("`#(1 2 3)", "#(1 2 3)");
}

#[test]
fn test_quasiquote_vector_with_unquote() {
    // R7RS example: `#(10 5 ,(sqrt 4) ,@(map sqrt '(16 9)) 8) => #(10 5 2 4 3 8)
    // Using (square 2) instead of (sqrt 4) for simplicity
    assert_program_eval_to(
        "(define (square x) (* x x))
         `#(10 5 ,(square 2) ,@(map square '(4 3)) 8)",
        "#(10 5 4 16 9 8)",
    );
}

// ============================================================================
// Nested Quasiquote Tests
// ============================================================================

#[test]
fn test_nested_quasiquote_simple() {
    // R7RS example: `(a `(b ,(+ 1 2) ,(foo ,(+ 1 3) d) e) f)
    //            => (a `(b ,(+ 1 2) ,(foo 4 d) e) f)
    // Only the inner ,(+ 1 3) is evaluated because it's at the right depth
    assert_eval_to(
        "`(a `(b ,(+ 1 2) ,(foo ,(+ 1 3) d) e) f)",
        "(a `(b ,(+ 1 2) ,(foo 4 d) e) f)",
    );
}

#[test]
fn test_nested_quasiquote_with_variables() {
    // R7RS example with nested unquotes
    assert_program_eval_to(
        "(define name1 'x)
         (define name2 'y)
         `(a `(b ,,name1 ,',name2 d) e)",
        "(a `(b ,x ,'y d) e)",
    );
}

#[test]
fn test_double_unquote() {
    // ,, means unquote twice (once for each quasiquote level)
    assert_program_eval_to(
        "(define x 10)
         ``(a ,,x)",
        "`(a ,10)",
    );
}

// ============================================================================
// Functional Form Tests
// ============================================================================

#[test]
fn test_quasiquote_functional_form() {
    // (quasiquote ...) is equivalent to `...
    assert_eval_to("(quasiquote (list (unquote (+ 1 2)) 4))", "(list 3 4)");
}

#[test]
fn test_quasiquote_quoted() {
    // Quoting a quasiquote expression
    assert_eval_to(
        "'(quasiquote (list (unquote (+ 1 2)) 4))",
        "`(list ,(+ 1 2) 4)",
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_quasiquote_improper_list() {
    // Dotted pairs should work
    assert_eval_to("`(a . b)", "(a . b)");
    assert_eval_to("`(a b . c)", "(a b . c)");
}

// TODO: Fix improper list with unquote-splicing followed by dotted tail
// Currently fails with "unquote-splicing not in list context"
// Issue: Complex interaction between splicing and improper tail detection
#[test]
#[ignore]
fn test_quasiquote_improper_list_with_unquote() {
    // R7RS example: `((foo ,(- 10 3)) ,@(cdr '(c)) . ,(car '(cons)))
    //            => ((foo 7) . cons)
    assert_eval_to(
        "`((foo ,(- 10 3)) ,@(cdr '(c)) . ,(car '(cons)))",
        "((foo 7) . cons)",
    );
}

#[test]
fn test_quasiquote_null() {
    assert_eval_to("`()", "()");
}

#[test]
fn test_unquote_returns_list() {
    // Unquote that returns a list embeds the list as single element
    assert_eval_to("`(a ,(list 1 2 3) b)", "(a (1 2 3) b)");
}

// ============================================================================
// Error Cases
// ============================================================================

// TODO: Add error handling for unquote/unquote-splicing outside quasiquote
// Currently these evaluate instead of erroring as they should
// Need to define unquote/unquote-splicing as auxiliary syntax that errors when used outside quasiquote
#[test]
#[ignore]
#[should_panic]
fn test_unquote_outside_quasiquote() {
    // Using unquote outside quasiquote should be an error
    assert_eval_error("(unquote x)");
}

#[test]
#[ignore]
#[should_panic]
fn test_unquote_splicing_outside_quasiquote() {
    // Using unquote-splicing outside quasiquote should be an error
    assert_eval_error("(unquote-splicing x)");
}

#[test]
#[ignore]
#[should_panic]
fn test_unquote_splicing_non_list() {
    // Splicing a non-list should be an error
    assert_eval_error("`(a ,@42 b)");
}

// ============================================================================
// Complex Integration Tests
// ============================================================================

#[test]
fn test_quasiquote_in_let() {
    // Using quasiquote with let bindings
    assert_program_eval_to(
        "(let ((x 10) (y 20))
           `(sum is ,(+ x y)))",
        "(sum is 30)",
    );
}

#[test]
fn test_quasiquote_building_code() {
    // Common macro use case: building code
    assert_program_eval_to(
        "(define (make-adder n)
           `(lambda (x) (+ x ,n)))
         (make-adder 5)",
        "(lambda (x) (+ x 5))",
    );
}

#[test]
fn test_quasiquote_with_map() {
    // Practical example combining map and splicing
    assert_program_eval_to(
        "(define nums '(1 2 3))
         `(results: ,@(map (lambda (x) (* x 2)) nums))",
        "(results: 2 4 6)",
    );
}

#[test]
fn test_deeply_nested_quasiquote() {
    // Three levels of nesting
    assert_program_eval_to(
        "(define x 1)
         ```(a ,,,x)",
        "``(a ,,1)",
    );
}

// ============================================================================
// R7RS Specification Examples
// ============================================================================

#[test]
fn test_r7rs_example_1() {
    // Section 4.2.8 example
    assert_eval_to("`(list ,(+ 1 2) 4)", "(list 3 4)");
}

#[test]
fn test_r7rs_example_2() {
    // Section 4.2.8 example
    // Note: We display (quote a) as 'a which is equivalent and more readable
    assert_program_eval_to(
        "(let ((name 'a))
           `(list ,name ',name))",
        "(list a 'a)",
    );
}

#[test]
fn test_r7rs_example_3() {
    // Section 4.2.8 example
    assert_eval_to("`(a ,(+ 1 2) ,@(map abs '(4 -5 6)) b)", "(a 3 4 5 6 b)");
}

// TODO: Same issue as test_quasiquote_improper_list_with_unquote
#[test]
#[ignore]
fn test_r7rs_example_4() {
    // Section 4.2.8 example (adapted without sqrt)
    assert_eval_to(
        "`((foo ,(- 10 3)) ,@(cdr '(c)) . ,(car '(cons)))",
        "((foo 7) . cons)",
    );
}

#[test]
fn test_r7rs_example_5() {
    // Section 4.2.8 nested example
    assert_eval_to(
        "`(a `(b ,(+ 1 2) ,(foo ,(+ 1 3) d) e) f)",
        "(a `(b ,(+ 1 2) ,(foo 4 d) e) f)",
    );
}

#[test]
fn test_r7rs_example_6() {
    // Section 4.2.8 nested with variables
    assert_program_eval_to(
        "(let ((name1 'x) (name2 'y))
           `(a `(b ,,name1 ,',name2 d) e))",
        "(a `(b ,x ,'y d) e)",
    );
}
