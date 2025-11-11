//! Advanced R7RS Macro Tests
//!
//! Tests for well-known, non-trivial macros that exercise the full
//! capabilities of the hygienic macro system.

use crate::common::*;

// ============================================================================
// Classic Control Flow Macros
// ============================================================================

#[test]
fn test_my_when_macro() {
    assert_program_eval_to(
        r#"
        (define-syntax my-when
          (syntax-rules ()
            ((my-when test body ...)
             (if test (begin body ...)))))

        (define result 0)
        (my-when (> 5 3)
          (set! result 10)
          (set! result (+ result 5)))
        result
        "#,
        "15",
    );
}

#[test]
fn test_my_unless_macro() {
    assert_program_eval_to(
        r#"
        (define-syntax my-unless
          (syntax-rules ()
            ((my-unless test body ...)
             (if (not test) (begin body ...)))))

        (define result 0)
        (my-unless (< 5 3)
          (set! result 20))
        result
        "#,
        "20",
    );
}

#[test]
fn test_cond_else_macro() {
    // Simplified cond using macros (testing macro-generating patterns)
    assert_program_eval_to(
        r#"
        (define-syntax my-cond
          (syntax-rules (else)
            ((my-cond (else result))
             result)
            ((my-cond (test result))
             (if test result))
            ((my-cond (test result) clause ...)
             (if test result (my-cond clause ...)))))

        (my-cond
          ((< 5 3) 'less)
          ((> 5 3) 'greater)
          (else 'equal))
        "#,
        "greater",
    );
}

// ============================================================================
// Binding Macros
// ============================================================================

#[test]
fn test_let_macro_implementation() {
    // Implement let in terms of lambda (classic macro example)
    assert_program_eval_to(
        r#"
        (define-syntax my-let
          (syntax-rules ()
            ((my-let ((var val) ...) body ...)
             ((lambda (var ...) body ...) val ...))))

        (my-let ((x 10) (y 20))
          (+ x y))
        "#,
        "30",
    );
}

#[test]
fn test_named_let_for_recursion() {
    // Named let for recursion
    assert_program_eval_to(
        r#"
        (define-syntax named-let
          (syntax-rules ()
            ((named-let name ((var val) ...) body ...)
             (letrec ((name (lambda (var ...) body ...)))
               (name val ...)))))

        (named-let loop ((n 5) (acc 1))
          (if (= n 0)
              acc
              (loop (- n 1) (* acc n))))
        "#,
        "120",
    );
}

// ============================================================================
// Mutation Macros
// ============================================================================

#[test]
fn test_push_macro() {
    // Classic push! macro (like Lisp)
    assert_program_eval_to(
        r#"
        (define-syntax push!
          (syntax-rules ()
            ((push! item lst)
             (set! lst (cons item lst)))))

        (define my-list '(2 3))
        (push! 1 my-list)
        my-list
        "#,
        "(1 2 3)",
    );
}

#[test]
fn test_inc_macro() {
    // Increment macro
    assert_program_eval_to(
        r#"
        (define-syntax inc!
          (syntax-rules ()
            ((inc! var)
             (set! var (+ var 1)))
            ((inc! var delta)
             (set! var (+ var delta)))))

        (define x 10)
        (inc! x)
        (inc! x 5)
        x
        "#,
        "16",
    );
}

#[test]
fn test_swap_macro_with_hygiene() {
    // The classic hygiene test - swap without capture
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
        "(2 1 999)",
    );
}

// ============================================================================
// Logic and Utility Macros
// ============================================================================

#[test]
fn test_and_macro() {
    // Short-circuit and
    assert_program_eval_to(
        r#"
        (define-syntax my-and
          (syntax-rules ()
            ((my-and) #t)
            ((my-and test) test)
            ((my-and test1 test2 ...)
             (if test1 (my-and test2 ...) #f))))

        (list
          (my-and (> 5 3) (< 2 4))
          (my-and (> 5 3) (> 2 4))
          (my-and))
        "#,
        "(#t #f #t)",
    );
}

#[test]
fn test_or_macro() {
    // Short-circuit or
    assert_program_eval_to(
        r#"
        (define-syntax my-or
          (syntax-rules ()
            ((my-or) #f)
            ((my-or test) test)
            ((my-or test1 test2 ...)
             (let ((temp test1))
               (if temp temp (my-or test2 ...))))))

        (list
          (my-or (< 5 3) (> 2 4) (= 1 1))
          (my-or (< 5 3) (> 2 4))
          (my-or))
        "#,
        "(#t #f #f)",
    );
}

#[test]
fn test_begin0_macro() {
    // Return first value but evaluate all
    assert_program_eval_to(
        r#"
        (define-syntax begin0
          (syntax-rules ()
            ((begin0 first rest ...)
             (let ((temp first))
               rest ...
               temp))))

        (define x 1)
        (begin0
          (+ x 10)
          (set! x 20)
          (set! x 30))
        "#,
        "11", // Returns (+ 1 10) even though x changed
    );
}

// ============================================================================
// List Processing Macros
// ============================================================================

#[test]
fn test_dotimes_macro() {
    // Loop n times
    assert_program_eval_to(
        r#"
        (define-syntax dotimes
          (syntax-rules ()
            ((dotimes (var count) body ...)
             (letrec ((loop (lambda (var)
                              (if (< var count)
                                  (begin
                                    body ...
                                    (loop (+ var 1)))))))
               (loop 0)))))

        (define sum 0)
        (dotimes (i 5)
          (set! sum (+ sum i)))
        sum
        "#,
        "10", // 0+1+2+3+4
    );
}

#[test]
fn test_while_macro() {
    // While loop
    assert_program_eval_to(
        r#"
        (define-syntax while
          (syntax-rules ()
            ((while test body ...)
             (letrec ((loop (lambda ()
                              (if test
                                  (begin
                                    body ...
                                    (loop))))))
               (loop)))))

        (define n 5)
        (define result 1)
        (while (> n 0)
          (set! result (* result n))
          (set! n (- n 1)))
        result
        "#,
        "120",
    );
}

// ============================================================================
// Pattern Matching and Destructuring
// ============================================================================

#[test]
fn test_let_values_simple() {
    // Simplified let-values (R7RS has this built-in but testing macro version)
    assert_program_eval_to(
        r#"
        (define-syntax simple-let-values
          (syntax-rules ()
            ((simple-let-values ((var ...)) expr body ...)
             (call-with-values
               (lambda () expr)
               (lambda (var ...) body ...)))))

        (simple-let-values ((a b))
          (values 10 20)
          (+ a b))
        "#,
        "30",
    );
}

// ============================================================================
// Ellipsis Tests (Complex Patterns)
// ============================================================================

#[test]
fn test_list_star_macro() {
    // list* - like cons but with multiple args
    assert_program_eval_to(
        r#"
        (define-syntax list*
          (syntax-rules ()
            ((list* last)
             last)
            ((list* first rest ...)
             (cons first (list* rest ...)))))

        (list* 1 2 3 '(4 5))
        "#,
        "(1 2 3 4 5)",
    );
}

#[test]
#[ignore] // TODO: See internal/NESTED_ELLIPSIS_LIMITATION.md - not yet supported
fn test_nested_ellipsis() {
    // Multiple items with nested ellipsis
    // This requires nested ellipsis support: (expr ...) ...
    // This IS part of R7RS but rarely used in practice
    assert_program_eval_to(
        r#"
        (define-syntax multi-begin
          (syntax-rules ()
            ((multi-begin (expr ...) ...)
             (begin expr ... ...))))

        (define x 0)
        (multi-begin
          ((set! x 1) (set! x (+ x 1)))
          ((set! x (+ x 10)) (set! x (+ x 20))))
        x
        "#,
        "33", // 1, 2, 12, 32, final: 33
    );
}

#[test]
fn test_repeat_expr_macro() {
    // Repeat an expression multiple times
    assert_program_eval_to(
        r#"
        (define-syntax repeat
          (syntax-rules ()
            ((repeat (times ...) expr)
             (begin times ... expr))))

        (define x 0)
        (repeat (#f #f #f) (set! x (+ x 1)))
        x
        "#,
        "1", // expr evaluated once, dummy times ignored
    );
}

// ============================================================================
// Hygiene Stress Tests
// ============================================================================

#[test]
fn test_triple_nested_macros() {
    // Three levels of macro nesting
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

        (define-syntax safe-div
          (syntax-rules ()
            ((safe-div a b default)
             (my-unless (= b 0)
               (my-when (> a 0)
                 (/ a b))))))

        (safe-div 10 2 0)
        "#,
        "5",
    );
}

#[test]
fn test_hygiene_with_multiple_temps() {
    // Multiple temporary variables - all should be renamed
    assert_program_eval_to(
        r#"
        (define-syntax complex-swap
          (syntax-rules ()
            ((complex-swap a b c)
             (let ((temp1 a) (temp2 b) (temp3 c))
               (set! a temp3)
               (set! b temp1)
               (set! c temp2)))))

        (define temp1 100)
        (define temp2 200)
        (define temp3 300)
        (define x 1)
        (define y 2)
        (define z 3)
        (complex-swap x y z)
        (list x y z temp1 temp2 temp3)
        "#,
        "(3 1 2 100 200 300)", // x,y,z rotated; temps unchanged
    );
}

#[test]
fn test_recursive_macro() {
    // Macro that expands into itself (like list construction)
    assert_program_eval_to(
        r#"
        (define-syntax build-list
          (syntax-rules ()
            ((build-list)
             '())
            ((build-list x)
             (cons x '()))
            ((build-list x y ...)
             (cons x (build-list y ...)))))

        (build-list 1 2 3 4 5)
        "#,
        "(1 2 3 4 5)",
    );
}

// ============================================================================
// Practical Macros
// ============================================================================

#[test]
fn test_assert_macro() {
    // Simple assertion macro with quoted symbols
    // Tests that hygiene doesn't rename inside quote forms
    assert_program_eval_to(
        r#"
        (define-syntax assert
          (syntax-rules ()
            ((assert test)
             (if (not test)
                 'assertion-failed
                 'ok))))

        (list
          (assert (= 2 2))
          (assert (> 5 3)))
        "#,
        "(ok ok)",
    );
}

#[test]
fn test_comment_macro() {
    // Comment macro - discards all arguments
    assert_program_eval_to(
        r#"
        (define-syntax comment
          (syntax-rules ()
            ((comment anything ...)
             (begin))))

        (define x 10)
        (comment
          (set! x 999)
          (set! x 'should-not-execute)
          this is all ignored)
        x
        "#,
        "10",
    );
}

#[test]
fn test_trace_macro() {
    // Macro that evaluates and returns (simplified tracing)
    assert_program_eval_to(
        r#"
        (define-syntax trace
          (syntax-rules ()
            ((trace expr)
             (let ((result expr))
               result))))

        (trace (+ 1 2 3))
        "#,
        "6",
    );
}

// ============================================================================
// Edge Cases and Complex Patterns
// ============================================================================

#[test]
fn test_macro_with_literals() {
    // Using literal identifiers (like '=>' as a literal keyword)
    assert_program_eval_to(
        r#"
        (define-syntax arrow-if
          (syntax-rules (=>)
            ((arrow-if test => then-expr)
             (if test then-expr #f))))

        (arrow-if (> 5 3) => 42)
        "#,
        "42",
    );
}

#[test]
fn test_empty_ellipsis() {
    // Ellipsis matching zero items
    assert_program_eval_to(
        r#"
        (define-syntax maybe-begin
          (syntax-rules ()
            ((maybe-begin body ...)
             (begin body ...))))

        (list
          (maybe-begin 42)
          (maybe-begin))
        "#,
        "(42 #<unspecified>)",
    );
}

#[test]
fn test_pattern_with_nested_structure() {
    // Pattern matching nested list structure
    assert_program_eval_to(
        r#"
        (define-syntax let-pair
          (syntax-rules ()
            ((let-pair ((a b) pair-expr) body ...)
             (let ((temp pair-expr))
               (let ((a (car temp))
                     (b (car (cdr temp))))
                 body ...)))))

        (let-pair ((x y) '(10 20))
          (+ x y))
        "#,
        "30",
    );
}
