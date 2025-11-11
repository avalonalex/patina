// Vector operations (R7RS Section 6.8)

use crate::common::*;

#[test]
fn test_vector_literal() {
    assert_eval_to("#()", "#()");
    assert_eval_to("#(1 2 3)", "#(1 2 3)");
    assert_eval_to("#(a b c)", "#(a b c)");
    assert_eval_to("#(0 (2 2 2 2) \"Anna\")", "#(0 (2 2 2 2) \"Anna\")");
}

#[test]
fn test_vector_predicate() {
    assert_eval_to("(vector? #())", "#t");
    assert_eval_to("(vector? #(1 2 3))", "#t");
    assert_eval_to("(vector? '#(1 2 3))", "#t");
    assert_eval_to("(vector? '())", "#f");
    assert_eval_to("(vector? 42)", "#f");
    assert_eval_to("(vector? \"string\")", "#f");
}

#[test]
fn test_make_vector() {
    assert_eval_to("(vector-length (make-vector 0))", "0");
    assert_eval_to("(vector-length (make-vector 5))", "5");
    assert_eval_to("(make-vector 3 'a)", "#(a a a)");
    assert_eval_to("(make-vector 0 'x)", "#()");
}

#[test]
fn test_vector_constructor() {
    assert_eval_to("(vector)", "#()");
    assert_eval_to("(vector 'a 'b 'c)", "#(a b c)");
    assert_eval_to("(vector 1 2 3 4 5)", "#(1 2 3 4 5)");
}

#[test]
fn test_vector_length() {
    assert_eval_to("(vector-length #())", "0");
    assert_eval_to("(vector-length #(a b c))", "3");
    assert_eval_to("(vector-length (make-vector 1000))", "1000");
}

#[test]
fn test_vector_ref() {
    assert_eval_to("(vector-ref '#(1 1 2 3 5 8 13 21) 5)", "8");
    assert_eval_to("(vector-ref '#(1 1 2 3 5 8 13 21) 0)", "1");
    assert_eval_to("(vector-ref '#(1 1 2 3 5 8 13 21) 7)", "21");
    assert_eval_to("(vector-ref '#(a b c) 1)", "b");
}

#[test]
fn test_vector_ref_errors() {
    assert_eval_error("(vector-ref '#(1 2 3) 3)"); // out of bounds
    assert_eval_error("(vector-ref '#(1 2 3) -1)"); // negative index
    assert_eval_error("(vector-ref '() 0)"); // not a vector
}

#[test]
fn test_vector_set() {
    assert_program_eval_to(
        "(define vec (vector 0 '(2 2 2 2) \"Anna\"))
         (vector-set! vec 1 '(\"Sue\" \"Sue\"))
         vec",
        "#(0 (\"Sue\" \"Sue\") \"Anna\")",
    );

    assert_program_eval_to(
        "(define v (make-vector 3 0))
         (vector-set! v 0 10)
         (vector-set! v 1 20)
         (vector-set! v 2 30)
         v",
        "#(10 20 30)",
    );
}

#[test]
fn test_vector_to_list() {
    assert_eval_to("(vector->list '#())", "()");
    assert_eval_to("(vector->list '#(dah dah didah))", "(dah dah didah)");
    assert_eval_to("(vector->list '#(dah dah didah) 1)", "(dah didah)");
    assert_eval_to("(vector->list '#(dah dah didah) 1 2)", "(dah)");
    assert_eval_to("(vector->list '#(a b c d) 0 4)", "(a b c d)");
    assert_eval_to("(vector->list '#(a b c d) 2 4)", "(c d)");
}

#[test]
fn test_list_to_vector() {
    assert_eval_to("(list->vector '())", "#()");
    assert_eval_to("(list->vector '(dididit dah))", "#(dididit dah)");
    assert_eval_to("(list->vector '(1 2 3 4 5))", "#(1 2 3 4 5)");
}

#[test]
fn test_vector_string_conversion() {
    // string->vector
    assert_eval_to("(string->vector \"\")", "#()");
    assert_eval_to("(string->vector \"ABC\")", "#(#\\A #\\B #\\C)");
    assert_eval_to("(string->vector \"ABC\" 1)", "#(#\\B #\\C)");
    assert_eval_to("(string->vector \"ABC\" 1 2)", "#(#\\B)");

    // vector->string
    assert_eval_to("(vector->string #())", "\"\"");
    assert_eval_to("(vector->string #(#\\1 #\\2 #\\3))", "\"123\"");
    assert_eval_to("(vector->string #(#\\a #\\b #\\c #\\d) 1)", "\"bcd\"");
    assert_eval_to("(vector->string #(#\\a #\\b #\\c #\\d) 1 3)", "\"bc\"");
}

#[test]
fn test_vector_copy() {
    assert_eval_to("(vector-copy #())", "#()");
    assert_eval_to("(vector-copy #(a b c))", "#(a b c)");
    assert_eval_to("(vector-copy #(a b c) 1)", "#(b c)");
    assert_eval_to("(vector-copy #(a b c) 1 2)", "#(b)");

    // Test that copy creates a new vector
    assert_program_eval_to(
        "(define a #(1 8 2 8))
         (define b (vector-copy a))
         (vector-set! b 0 3)
         (list a b)",
        "(#(1 8 2 8) #(3 8 2 8))",
    );
}

#[test]
fn test_vector_copy_bang() {
    assert_program_eval_to(
        "(define a (vector 1 2 3 4 5))
         (define b (vector 10 20 30 40 50))
         (vector-copy! b 1 a 0 2)
         b",
        "#(10 1 2 40 50)",
    );

    assert_program_eval_to(
        "(define v (vector 1 2 3 4 5))
         (vector-copy! v 0 v 2 5)
         v",
        "#(3 4 5 4 5)",
    );
}

#[test]
fn test_vector_append() {
    assert_eval_to("(vector-append)", "#()");
    assert_eval_to("(vector-append #())", "#()");
    assert_eval_to("(vector-append #() #())", "#()");
    assert_eval_to("(vector-append #() #(a b c))", "#(a b c)");
    assert_eval_to("(vector-append #(a b c) #())", "#(a b c)");
    assert_eval_to("(vector-append #(a b c) #(d e))", "#(a b c d e)");
    assert_eval_to("(vector-append #(1 2) #(3 4) #(5 6))", "#(1 2 3 4 5 6)");
}

#[test]
fn test_vector_fill() {
    assert_program_eval_to(
        "(define a (vector 1 2 3 4 5))
         (vector-fill! a 'smash 2 4)
         a",
        "#(1 2 smash smash 5)",
    );

    assert_program_eval_to(
        "(define v (make-vector 5 0))
         (vector-fill! v 'x)
         v",
        "#(x x x x x)",
    );

    assert_program_eval_to(
        "(define v (vector 1 2 3 4 5))
         (vector-fill! v 99 1 3)
         v",
        "#(1 99 99 4 5)",
    );
}

#[test]
fn test_vector_map() {
    assert_program_eval_to(
        "(vector-map (lambda (x) (* x x)) #(1 2 3 4))",
        "#(1 4 9 16)",
    );

    assert_program_eval_to("(vector-map + #(1 2 3) #(4 5 6))", "#(5 7 9)");

    assert_program_eval_to("(vector-map + #(1 2 3) #(4 5 6 7))", "#(5 7 9)"); // Shortest vector determines length

    assert_eval_to("(vector-map + #() #())", "#()");
}

#[test]
fn test_vector_for_each() {
    assert_program_eval_to(
        "(define sum 0)
         (vector-for-each (lambda (x) (set! sum (+ sum x))) #(1 2 3 4))
         sum",
        "10",
    );

    assert_program_eval_to(
        "(define result '())
         (vector-for-each (lambda (x y) (set! result (cons (+ x y) result)))
                          #(1 2 3) #(10 20 30))
         (reverse result)",
        "(11 22 33)",
    );
}

#[test]
fn test_vector_equality() {
    assert_eval_to("(equal? #() #())", "#t");
    assert_eval_to("(equal? #(1 2 3) #(1 2 3))", "#t");
    assert_eval_to("(equal? #(a b c) #(a b c))", "#t");
    assert_eval_to("(equal? #(1 2 3) #(1 2 4))", "#f");
    assert_eval_to("(equal? #(1 2) #(1 2 3))", "#f");

    // Nested structures
    assert_eval_to("(equal? #(#(1 2) #(3 4)) #(#(1 2) #(3 4)))", "#t");
    assert_eval_to("(equal? #((a b) (c d)) #((a b) (c d)))", "#t");
}

#[test]
fn test_vector_mixed_operations() {
    // Test round-trip conversions
    assert_program_eval_to("(list->vector (vector->list #(a b c)))", "#(a b c)");

    assert_program_eval_to("(vector->list (list->vector '(1 2 3)))", "(1 2 3)");

    assert_program_eval_to(
        "(string->vector (vector->string #(#\\x #\\y #\\z)))",
        "#(#\\x #\\y #\\z)",
    );
}

#[test]
fn test_vector_as_self_evaluating() {
    // Vectors are self-evaluating (don't need quoting)
    assert_eval_to("#(1 2 3)", "#(1 2 3)");
    assert_eval_to("#(a b c)", "#(a b c)"); // Symbols in vector literal
}
