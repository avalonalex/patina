//! R7RS Section 6.4 - Pairs and lists
//!
//! Tests based on chibi-scheme's r7rs-tests.scm
//! Reference: ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

use super::common::*;

// 6.4 Pairs and lists - Constructors
#[test]
fn test_cons() {
    assert_eval_to("(cons 1 2)", "(1 . 2)");
    assert_eval_to("(cons 'a 'b)", "(a . b)");
}

#[test]
fn test_cons_list() {
    assert_eval_to("(cons 1 '())", "(1)");
    assert_eval_to("(cons 1 (cons 2 '()))", "(1 2)");
}

// 6.4 Pairs and lists - Accessors
#[test]
fn test_car() {
    assert_eval_to("(car (cons 1 2))", "1");
    assert_eval_to("(car '(a b c))", "a");
}

#[test]
fn test_cdr() {
    assert_eval_to("(cdr (cons 1 2))", "2");
    assert_eval_to("(cdr '(a b c))", "(b c)");
}

// 6.4 Pairs and lists - Predicates
#[test]
fn test_null_predicate() {
    assert_eval_to("(null? '())", "#t");
    assert_eval_to("(null? '(a))", "#f");
    assert_eval_to("(null? 42)", "#f");
}

#[test]
fn test_pair_predicate() {
    assert_eval_to("(pair? (cons 1 2))", "#t");
    assert_eval_to("(pair? '(a b c))", "#t");
    assert_eval_to("(pair? '())", "#f");
    assert_eval_to("(pair? 42)", "#f");
}

// 6.4 Pairs and lists - List operations
#[test]
fn test_list() {
    assert_eval_to("(list)", "()");
    assert_eval_to("(list 1)", "(1)");
    assert_eval_to("(list 1 2 3)", "(1 2 3)");
}

#[test]
fn test_list_predicate() {
    assert_eval_to("(list? '())", "#t");
    assert_eval_to("(list? '(1 2 3))", "#t");
    assert_eval_to("(list? '(1 . 2))", "#f");
    assert_eval_to("(list? 42)", "#f");
}

#[test]
fn test_length() {
    assert_eval_to("(length '())", "0");
    assert_eval_to("(length '(a))", "1");
    assert_eval_to("(length '(a b c))", "3");
}

#[test]
fn test_append() {
    assert_eval_to("(append '(a) '(b c))", "(a b c)");
    assert_eval_to("(append '() '(a b))", "(a b)");
    assert_eval_to("(append '(a b) '())", "(a b)");
    assert_eval_to("(append '(a b) '(c d) '(e f))", "(a b c d e f)");
}

#[test]
fn test_reverse() {
    assert_eval_to("(reverse '())", "()");
    assert_eval_to("(reverse '(a))", "(a)");
    assert_eval_to("(reverse '(a b c))", "(c b a)");
}

#[test]
fn test_list_ref() {
    assert_eval_to("(list-ref '(a b c d) 0)", "a");
    assert_eval_to("(list-ref '(a b c d) 2)", "c");
}

#[test]
fn test_list_tail() {
    assert_eval_to("(list-tail '(a b c d) 0)", "(a b c d)");
    assert_eval_to("(list-tail '(a b c d) 2)", "(c d)");
}

// 6.4 Pairs and lists - cXr combinations
#[test]
fn test_caar() {
    assert_eval_to("(caar '((a b) c d))", "a");
}

#[test]
fn test_cadr() {
    assert_eval_to("(cadr '(a b c d))", "b");
}

#[test]
fn test_cdar() {
    assert_eval_to("(cdar '((a b) c d))", "(b)");
}

#[test]
fn test_cddr() {
    assert_eval_to("(cddr '(a b c d))", "(c d)");
}

// 6.4 Pairs and lists - Association lists
#[test]
#[ignore] // TODO: Implement assq
fn test_assq() {
    assert_eval_to("(assq 'b '((a 1) (b 2) (c 3)))", "(b 2)");
    assert_eval_to("(assq 'd '((a 1) (b 2) (c 3)))", "#f");
}

#[test]
#[ignore] // TODO: Implement memq
fn test_memq() {
    assert_eval_to("(memq 'b '(a b c))", "(b c)");
    assert_eval_to("(memq 'd '(a b c))", "#f");
}

// 6.4 Higher-order list operations
#[test]
fn test_map_single_list() {
    // Basic map with single list
    assert_eval_to("(map (lambda (x) (* x 2)) '(1 2 3))", "(2 4 6)");
}

#[test]
fn test_map_with_primitive() {
    // map with primitive procedure (needs cadr)
    // For now test with car
    assert_eval_to("(map car '((1 2) (3 4) (5 6)))", "(1 3 5)");
}

#[test]
fn test_map_multiple_lists() {
    // map with multiple lists
    assert_eval_to("(map + '(1 2 3) '(4 5 6))", "(5 7 9)");
}

#[test]
fn test_map_shortest_list() {
    // map terminates on shortest list
    assert_eval_to("(map + '(1 2 3) '(4 5 6 7))", "(5 7 9)");
}

#[test]
fn test_map_empty_list() {
    // map with empty list
    assert_eval_to("(map (lambda (x) (* x 2)) '())", "()");
}

#[test]
fn test_for_each_side_effects() {
    // for-each with side effects
    assert_program_eval_to(
        r#"
        (define result 0)
        (for-each (lambda (x) (set! result (+ result x))) '(1 2 3 4))
        result
        "#,
        "10",
    );
}

#[test]
fn test_for_each_multiple_lists() {
    // for-each with multiple lists
    assert_program_eval_to(
        r#"
        (define sum 0)
        (for-each (lambda (x y) (set! sum (+ sum x y))) '(1 2 3) '(4 5 6))
        sum
        "#,
        "21",
    );
}
