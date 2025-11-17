//! Tests for (scheme lazy) - R7RS lazy evaluation
//!
//! Tests delay, delay-force, force, promise?, and make-promise

mod common;

use common::*;

// ========== Basic delay/force Tests ==========

#[test]
fn test_delay_force_basic() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p (delay (+ 1 2)))
        (force p)
        "#,
        "3",
    );
}

#[test]
fn test_delay_evaluates_lazily() {
    // The delayed expression should not be evaluated until forced
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define x 10)
        (define p (delay (set! x 20)))
        x  ; Should still be 10 (not forced yet)
        "#,
        "10",
    );
}

#[test]
fn test_force_evaluates_delayed_expression() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define x 10)
        (define p (delay (set! x 20)))
        (force p)
        x  ; Should now be 20
        "#,
        "20",
    );
}

#[test]
fn test_force_caches_result() {
    // Forcing a promise multiple times should only evaluate once
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define count 0)
        (define p (delay (begin (set! count (+ count 1)) count)))
        (force p)
        (force p)
        (force p)
        count  ; Should be 1 (only evaluated once)
        "#,
        "1",
    );
}

#[test]
fn test_force_on_non_promise() {
    // Forcing a non-promise should return it unchanged
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (force 42)
        "#,
        "42",
    );
}

// ========== promise? Predicate Tests ==========

#[test]
fn test_promise_predicate_on_promise() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p (delay 1))
        (promise? p)
        "#,
        "#t",
    );
}

#[test]
fn test_promise_predicate_on_non_promise() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (promise? 42)
        "#,
        "#f",
    );
}

#[test]
fn test_promise_predicate_on_forced_promise() {
    // A promise is still a promise even after being forced
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p (delay 1))
        (force p)
        (promise? p)
        "#,
        "#t",
    );
}

// ========== make-promise Tests ==========

#[test]
fn test_make_promise_from_value() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p (make-promise 42))
        (force p)
        "#,
        "42",
    );
}

#[test]
fn test_make_promise_idempotent() {
    // make-promise on a promise should return the same promise
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p1 (delay 1))
        (define p2 (make-promise p1))
        (promise? p2)
        "#,
        "#t",
    );
}

// ========== delay-force Tests ==========

#[test]
fn test_delay_force_with_nested_delay() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p (delay-force (delay 42)))
        (force p)
        "#,
        "42",
    );
}

#[test]
fn test_delay_force_recursive() {
    // delay-force should recursively force nested promises
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p (delay-force (delay-force (delay 42))))
        (force p)
        "#,
        "42",
    );
}

// ========== Lazy Data Structures Tests ==========

#[test]
fn test_lazy_list_head() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define (lazy-range n)
          (delay
            (if (= n 0)
                '()
                (cons n (lazy-range (- n 1))))))

        (define lst (force (lazy-range 5)))
        (car lst)
        "#,
        "5",
    );
}

#[test]
fn test_lazy_list_tail_is_promise() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define (lazy-range n)
          (delay
            (if (= n 0)
                '()
                (cons n (lazy-range (- n 1))))))

        (define lst (force (lazy-range 5)))
        (promise? (cdr lst))
        "#,
        "#t",
    );
}

#[test]
fn test_lazy_list_force_multiple_elements() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define (lazy-range n)
          (delay
            (if (= n 0)
                '()
                (cons n (lazy-range (- n 1))))))

        (define lst (force (lazy-range 3)))
        (define second (car (force (cdr lst))))
        second
        "#,
        "2",
    );
}

// ========== Lazy Fibonacci Sequence Tests ==========

#[test]
fn test_lazy_fibonacci() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))

        (define (fib-gen a b)
          (delay (cons a (fib-gen b (+ a b)))))

        (define fibs (fib-gen 0 1))

        ; Get first fibonacci number
        (define fib0 (car (force fibs)))

        ; Get second fibonacci number
        (define rest1 (cdr (force fibs)))
        (define fib1 (car (force rest1)))

        ; Get third fibonacci number
        (define rest2 (cdr (force rest1)))
        (define fib2 (car (force rest2)))

        (list fib0 fib1 fib2)
        "#,
        "(0 1 1)",
    );
}

// ========== Error Cases and Edge Cases ==========

#[test]
fn test_force_evaluates_thunk() {
    // The delayed expression is wrapped in a thunk (zero-arg lambda)
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define counter 0)
        (define (increment) (set! counter (+ counter 1)) counter)
        (define p (delay (increment)))
        (force p)
        "#,
        "1",
    );
}

#[test]
fn test_nested_delays() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p1 (delay (+ 1 2)))
        (define p2 (delay (force p1)))
        (force p2)
        "#,
        "3",
    );
}

#[test]
fn test_delay_with_complex_expression() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p (delay
          (let ((x 10)
                (y 20))
            (+ x y (* x y)))))
        (force p)
        "#,
        "230",
    );
}

// ========== R7RS Specification Examples ==========

#[test]
fn test_r7rs_example_1() {
    // From R7RS Section 6.10:
    // (force (delay (+ 1 2))) => 3
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (force (delay (+ 1 2)))
        "#,
        "3",
    );
}

#[test]
fn test_r7rs_example_2() {
    // From R7RS: promise? returns #t for promises
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define p (delay (+ 1 2)))
        (promise? p)
        "#,
        "#t",
    );
}

#[test]
fn test_r7rs_example_3() {
    // From R7RS: forcing a non-promise returns it
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (force 5)
        "#,
        "5",
    );
}

#[test]
fn test_r7rs_example_4() {
    // From R7RS: make-promise wraps a value
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (promise? (make-promise 5))
        "#,
        "#t",
    );
}

// ========== Lazy Filter Example ==========

#[test]
fn test_lazy_filter_predicate() {
    assert_program_eval_to(
        r#"
        (import (scheme lazy))

        (define (lazy-filter pred lst)
          (delay
            (if (null? lst)
                '()
                (let ((head (car lst))
                      (tail (cdr lst)))
                  (if (pred head)
                      (cons head (lazy-filter pred tail))
                      (force (lazy-filter pred tail)))))))

        (define evens (lazy-filter even? '(1 2 3 4 5 6)))
        (define result (force evens))
        (car result)  ; Should be 2 (first even number)
        "#,
        "2",
    );
}

// ========== Performance/Memory Tests ==========

#[test]
fn test_force_doesnt_re_evaluate() {
    // Verify memoization: forcing multiple times shouldn't re-evaluate
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define eval-count 0)
        (define p (delay (begin
                          (set! eval-count (+ eval-count 1))
                          42)))
        (force p)
        (force p)
        (force p)
        eval-count  ; Should be 1
        "#,
        "1",
    );
}

#[test]
fn test_delay_in_list() {
    // Promises can be stored in data structures
    assert_program_eval_to(
        r#"
        (import (scheme lazy))
        (define promises (list (delay 1) (delay 2) (delay 3)))
        (define first-promise (car promises))
        (force first-promise)
        "#,
        "1",
    );
}
