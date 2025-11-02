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
#[ignore] // TODO: Implement list
fn test_list() {
    assert_eval_to("(list)", "()");
    assert_eval_to("(list 1)", "(1)");
    assert_eval_to("(list 1 2 3)", "(1 2 3)");
}

#[test]
#[ignore] // TODO: Implement list?
fn test_list_predicate() {
    assert_eval_to("(list? '())", "#t");
    assert_eval_to("(list? '(1 2 3))", "#t");
    assert_eval_to("(list? '(1 . 2))", "#f");
}

#[test]
#[ignore] // TODO: Implement length
fn test_length() {
    assert_eval_to("(length '())", "0");
    assert_eval_to("(length '(a))", "1");
    assert_eval_to("(length '(a b c))", "3");
}

#[test]
#[ignore] // TODO: Implement append
fn test_append() {
    assert_eval_to("(append '(a) '(b c))", "(a b c)");
    assert_eval_to("(append '() '(a b))", "(a b)");
    assert_eval_to("(append '(a b) '())", "(a b)");
}

#[test]
#[ignore] // TODO: Implement reverse
fn test_reverse() {
    assert_eval_to("(reverse '())", "()");
    assert_eval_to("(reverse '(a))", "(a)");
    assert_eval_to("(reverse '(a b c))", "(c b a)");
}

#[test]
#[ignore] // TODO: Implement list-ref
fn test_list_ref() {
    assert_eval_to("(list-ref '(a b c d) 0)", "a");
    assert_eval_to("(list-ref '(a b c d) 2)", "c");
}

#[test]
#[ignore] // TODO: Implement list-tail
fn test_list_tail() {
    assert_eval_to("(list-tail '(a b c d) 0)", "(a b c d)");
    assert_eval_to("(list-tail '(a b c d) 2)", "(c d)");
}

// 6.4 Pairs and lists - cXr combinations
#[test]
#[ignore] // TODO: Implement caar, cadr, etc.
fn test_caar() {
    assert_eval_to("(caar '((a b) c d))", "a");
}

#[test]
#[ignore] // TODO: Implement caar, cadr, etc.
fn test_cadr() {
    assert_eval_to("(cadr '(a b c d))", "b");
}

#[test]
#[ignore] // TODO: Implement caar, cadr, etc.
fn test_cdar() {
    assert_eval_to("(cdar '((a b) c d))", "(b)");
}

#[test]
#[ignore] // TODO: Implement caar, cadr, etc.
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
