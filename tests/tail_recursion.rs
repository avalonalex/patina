//! Tests for tail call optimization in special forms
//!
//! R7RS Section 3.5 requires proper tail recursion - tail calls must execute
//! in constant stack space. These tests verify that all special forms correctly
//! handle tail positions and enable deep recursion without stack overflow.

mod common;
use common::*;

// =============================================================================
// If - Both branches are in tail position
// =============================================================================

#[test]
fn test_if_then_branch_tail_recursive() {
    // Countdown using if then-branch (tail recursive)
    assert_program_eval_to(
        r#"
        (define (countdown n)
          (if (= n 0)
              'done
              (countdown (- n 1))))

        (countdown 1000)
        "#,
        "done",
    );
}

#[test]
fn test_if_else_branch_tail_recursive() {
    // Countdown using if else-branch (tail recursive)
    assert_program_eval_to(
        r#"
        (define (countup n limit)
          (if (= n limit)
              n
              (countup (+ n 1) limit)))

        (countup 0 1000)
        "#,
        "1000",
    );
}

#[test]
fn test_if_nested_tail_recursive() {
    // Nested if expressions, innermost is tail position
    assert_program_eval_to(
        r#"
        (define (classify n)
          (if (= n 0)
              'zero
              (if (< n 0)
                  (classify (+ n 1))
                  (classify (- n 1)))))

        (classify 500)
        "#,
        "zero",
    );
}

// =============================================================================
// Begin - Last expression is in tail position
// =============================================================================

#[test]
fn test_begin_last_expr_tail_recursive() {
    // Begin with multiple expressions, last is tail recursive
    assert_program_eval_to(
        r#"
        (define (countdown-with-side-effects n)
          (begin
            (+ n 1)  ; Not tail position
            (* n 2)  ; Not tail position
            (if (= n 0)
                'done
                (countdown-with-side-effects (- n 1)))))

        (countdown-with-side-effects 1000)
        "#,
        "done",
    );
}

// =============================================================================
// Cond - Clause bodies are in tail position
// =============================================================================

#[test]
fn test_cond_clause_tail_recursive() {
    // Cond with tail recursive calls in different clauses
    assert_program_eval_to(
        r#"
        (define (collatz n)
          (cond
            ((= n 1) 1)
            ((= (remainder n 2) 0)
             (collatz (/ n 2)))
            (else
             (collatz (+ (* 3 n) 1)))))

        (collatz 27)  ; Takes 111 steps to reach 1
        "#,
        "1",
    );
}

#[test]
fn test_cond_multiple_clauses_tail_recursive() {
    // Multiple cond clauses with tail calls
    assert_program_eval_to(
        r#"
        (define (categorize n)
          (cond
            ((< n 0) (categorize (+ n 1000)))
            ((= n 0) 'zero)
            ((< n 100) (categorize (- n 10)))
            (else (categorize (- n 100)))))

        (categorize 1500)
        "#,
        "zero",
    );
}

// =============================================================================
// And - Last expression is in tail position
// =============================================================================

#[test]
fn test_and_last_expr_tail_recursive() {
    // And with tail recursive last expression
    assert_program_eval_to(
        r#"
        (define (all-positive? lst)
          (and
            (pair? lst)
            (> (car lst) 0)
            (if (null? (cdr lst))
                #t
                (all-positive? (cdr lst)))))

        (define test-list '(1 2 3 4 5 6 7 8 9 10))
        (all-positive? test-list)
        "#,
        "#t",
    );
}

#[test]
fn test_and_tail_recursive_countdown() {
    // And used in tail recursive countdown pattern
    assert_program_eval_to(
        r#"
        (define (countdown-and n)
          (and
            (> n 0)
            (countdown-and (- n 1))))

        (countdown-and 1000)
        "#,
        "#f", // Returns #f when n reaches 0
    );
}

// =============================================================================
// Or - Last expression is in tail position
// =============================================================================

#[test]
fn test_or_last_expr_tail_recursive() {
    // Or with tail recursive last expression
    assert_program_eval_to(
        r#"
        (define (find-zero? lst)
          (or
            (null? lst)
            (= (car lst) 0)
            (find-zero? (cdr lst))))

        (define test-list '(1 2 3 4 5 0 7 8))
        (find-zero? test-list)
        "#,
        "#t",
    );
}

#[test]
fn test_or_tail_recursive_search() {
    // Or used for tail recursive search
    assert_program_eval_to(
        r#"
        (define (search-value n target)
          (or
            (= n target)
            (> n 1000)
            (search-value (+ n 1) target)))

        (search-value 0 500)
        "#,
        "#t",
    );
}

// =============================================================================
// Let - Body is in tail position
// =============================================================================

#[test]
fn test_let_body_tail_recursive() {
    // Let with tail recursive body
    assert_program_eval_to(
        r#"
        (define (factorial n)
          (let ((acc 1))
            (define (iter n acc)
              (if (= n 0)
                  acc
                  (iter (- n 1) (* n acc))))
            (iter n acc)))

        (factorial 10)
        "#,
        "3628800",
    );
}

#[test]
fn test_let_nested_tail_recursive() {
    // Nested let with tail recursive inner body
    assert_program_eval_to(
        r#"
        (define (sum-range start end)
          (let ((sum 0))
            (let ((current start))
              (define (iter cur acc)
                (if (> cur end)
                    acc
                    (iter (+ cur 1) (+ acc cur))))
              (iter current sum))))

        (sum-range 1 100)
        "#,
        "5050",
    );
}

// =============================================================================
// Let* - Body is in tail position
// =============================================================================

#[test]
fn test_let_star_body_tail_recursive() {
    // Let* with tail recursive body
    assert_program_eval_to(
        r#"
        (define (gcd-iter a b)
          (let* ((r (remainder a b)))
            (if (= r 0)
                b
                (gcd-iter b r))))

        (gcd-iter 1071 462)
        "#,
        "21",
    );
}

#[test]
fn test_let_star_sequential_bindings_tail_recursive() {
    // Let* with sequential bindings and tail recursive body
    assert_program_eval_to(
        r#"
        (define (power base exp)
          (let* ((result 1)
                 (counter exp))
            (define (iter n acc)
              (if (= n 0)
                  acc
                  (iter (- n 1) (* acc base))))
            (iter counter result)))

        (power 2 10)
        "#,
        "1024",
    );
}

// =============================================================================
// Complex combinations - Multiple special forms with tail recursion
// =============================================================================

#[test]
fn test_combined_if_cond_tail_recursive() {
    // Combination of if and cond, both in tail positions
    assert_program_eval_to(
        r#"
        (define (complex-recurse n)
          (if (< n 0)
              (complex-recurse (+ n 100))
              (cond
                ((= n 0) 'done)
                ((< n 50) (complex-recurse (- n 1)))
                (else (complex-recurse (- n 10))))))

        (complex-recurse 500)
        "#,
        "done",
    );
}

#[test]
fn test_combined_let_and_or_tail_recursive() {
    // Let with and/or in tail positions
    assert_program_eval_to(
        r#"
        (define (validate-and-process n)
          (let ((threshold 100))
            (and
              (> n 0)
              (or
                (= n 1)
                (validate-and-process (- n 1))))))

        (validate-and-process 500)
        "#,
        "#t",
    );
}

#[test]
fn test_deeply_nested_tail_recursive() {
    // Deeply nested special forms all maintaining tail recursion
    assert_program_eval_to(
        r#"
        (define (nested-countdown n)
          (begin
            (let ((x n))
              (if (> x 0)
                  (begin
                    (cond
                      ((= x 1) 'done)
                      (else (nested-countdown (- x 1)))))
                  'done))))

        (nested-countdown 1000)
        "#,
        "done",
    );
}

// =============================================================================
// Stress tests - Ensure no stack overflow with deep recursion
// =============================================================================

#[test]
fn test_if_stress_10k_iterations() {
    // 10,000 tail recursive if calls
    assert_program_eval_to(
        r#"
        (define (countdown n)
          (if (= n 0) 'done (countdown (- n 1))))

        (countdown 10000)
        "#,
        "done",
    );
}

#[test]
fn test_cond_stress_10k_iterations() {
    // 10,000 tail recursive cond calls
    assert_program_eval_to(
        r#"
        (define (countdown n)
          (cond
            ((= n 0) 'done)
            (else (countdown (- n 1)))))

        (countdown 10000)
        "#,
        "done",
    );
}

#[test]
fn test_let_stress_10k_iterations() {
    // 10,000 tail recursive let calls
    assert_program_eval_to(
        r#"
        (define (countdown n)
          (let ((next (- n 1)))
            (if (= n 0)
                'done
                (countdown next))))

        (countdown 10000)
        "#,
        "done",
    );
}

#[test]
fn test_and_or_stress_5k_iterations() {
    // 5,000 tail recursive or calls (and short-circuits at first #f)
    assert_program_eval_to(
        r#"
        (define (countdown-or n)
          (or (= n 0) (countdown-or (- n 1))))

        (countdown-or 5000)
        "#,
        "#t",
    );
}

// =============================================================================
// Mutual recursion through special forms
// =============================================================================

#[test]
fn test_mutual_recursion_through_if() {
    // Mutual recursion using if in tail position
    assert_program_eval_to(
        r#"
        (define (even? n)
          (if (= n 0) #t (odd? (- n 1))))

        (define (odd? n)
          (if (= n 0) #f (even? (- n 1))))

        (even? 5000)
        "#,
        "#t",
    );
}

#[test]
fn test_mutual_recursion_through_cond() {
    // Mutual recursion using cond in tail position
    assert_program_eval_to(
        r#"
        (define (ping n)
          (cond
            ((= n 0) 'done)
            (else (pong (- n 1)))))

        (define (pong n)
          (cond
            ((= n 0) 'done)
            (else (ping (- n 1)))))

        (ping 5000)
        "#,
        "done",
    );
}

// =============================================================================
// Letrec - Body is in tail position
// =============================================================================

#[test]
fn test_letrec_body_tail_recursive() {
    // Letrec with mutually recursive functions in tail position
    assert_program_eval_to(
        r#"
        (letrec ((countdown (lambda (n)
                              (if (= n 0)
                                  'done
                                  (countdown (- n 1))))))
          (countdown 1000))
        "#,
        "done",
    );
}

#[test]
fn test_letrec_mutual_recursion_tail() {
    // Letrec enables mutual recursion with tail calls
    assert_program_eval_to(
        r#"
        (letrec ((even? (lambda (n)
                          (if (= n 0) #t (odd? (- n 1)))))
                 (odd? (lambda (n)
                         (if (= n 0) #f (even? (- n 1))))))
          (even? 5000))
        "#,
        "#t",
    );
}

// =============================================================================
// Letrec* - Body is in tail position
// =============================================================================

#[test]
fn test_letrec_star_body_tail_recursive() {
    // Letrec* with sequential bindings and tail recursive body
    assert_program_eval_to(
        r#"
        (letrec* ((countdown (lambda (n)
                               (if (= n 0)
                                   'done
                                   (countdown (- n 1))))))
          (countdown 1000))
        "#,
        "done",
    );
}

// =============================================================================
// Let-values - Body is in tail position
// =============================================================================

#[test]
fn test_let_values_body_tail_recursive() {
    // Let-values with tail recursive body
    assert_program_eval_to(
        r#"
        (define (countdown n acc)
          (let-values (((a b) (values n acc)))
            (if (= a 0)
                b
                (countdown (- a 1) (+ b 1)))))

        (countdown 1000 0)
        "#,
        "1000",
    );
}

#[test]
fn test_let_values_fibonacci_tail() {
    // Original fibonacci pattern with let-values
    assert_program_eval_to(
        r#"
        (define (fib n)
          (define (fib-iter a b count)
            (let-values (((x y) (values a b)))
              (if (= count 0)
                  x
                  (fib-iter y (+ x y) (- count 1)))))
          (fib-iter 0 1 n))

        (fib 100)
        "#,
        "354224848179261915075",
    );
}

// =============================================================================
// Let*-values - Body is in tail position
// =============================================================================

#[test]
fn test_let_star_values_body_tail_recursive() {
    // Let*-values with sequential bindings and tail recursive body
    assert_program_eval_to(
        r#"
        (define (countdown n)
          (let*-values (((x) (values n)))
            (if (= x 0)
                'done
                (countdown (- x 1)))))

        (countdown 1000)
        "#,
        "done",
    );
}

// =============================================================================
// Case - Clause bodies are in tail position
// =============================================================================

#[test]
fn test_case_clause_tail_recursive() {
    // Case with tail recursive calls in different clauses
    assert_program_eval_to(
        r#"
        (define (classify n)
          (case (remainder n 3)
            ((0) (if (= n 0) 'zero (classify (- n 3))))
            ((1) (if (= n 1) 'one (classify (- n 3))))
            ((2) (if (= n 2) 'two (classify (- n 3))))))

        (classify 3000)
        "#,
        "zero",
    );
}

#[test]
fn test_case_else_clause_tail_recursive() {
    // Case with else clause containing tail call
    assert_program_eval_to(
        r#"
        (define (countdown n)
          (case n
            ((0) 'done)
            (else (countdown (- n 1)))))

        (countdown 1000)
        "#,
        "done",
    );
}

#[test]
fn test_case_stress_5k_iterations() {
    // Stress test with 5,000 tail recursive case calls
    assert_program_eval_to(
        r#"
        (define (test-case n)
          (case (remainder n 2)
            ((0) (if (= n 0) 'even-zero (test-case (- n 2))))
            ((1) (if (= n 1) 'odd-one (test-case (- n 2))))))

        (test-case 5000)
        "#,
        "even-zero",
    );
}

// =============================================================================
// Do - Exit clause is in tail position
// =============================================================================

#[test]
fn test_do_exit_clause_tail_recursive() {
    // Do loop with tail recursive call in exit clause
    assert_program_eval_to(
        r#"
        (define (helper n)
          (if (= n 0) 'done (helper (- n 1))))

        (do ((i 0 (+ i 1)))
            ((= i 1) (helper 1000)))
        "#,
        "done",
    );
}

#[test]
fn test_do_loop_with_tail_exit() {
    // Do loop that exits with a tail recursive result expression
    assert_program_eval_to(
        r#"
        (define (factorial-iter n acc)
          (if (= n 0)
              acc
              (factorial-iter (- n 1) (* n acc))))

        (do ((x 10))
            ((= x 10) (factorial-iter x 1)))
        "#,
        "3628800",
    );
}

// =============================================================================
// Combined tests - Multiple new forms together
// =============================================================================

#[test]
fn test_letrec_case_combined_tail() {
    // Letrec with case in tail position
    assert_program_eval_to(
        r#"
        (letrec ((classify (lambda (n)
                             (case (remainder n 2)
                               ((0) (if (= n 0) 'zero (classify (- n 2))))
                               ((1) (if (= n 1) 'one (classify (- n 2))))))))
          (classify 1000))
        "#,
        "zero",
    );
}

#[test]
fn test_let_values_case_combined_tail() {
    // Let-values with case in tail position
    assert_program_eval_to(
        r#"
        (define (test n)
          (let-values (((x y) (values n 0)))
            (case (remainder x 3)
              ((0) (if (= x 0) 'done (test (- x 3))))
              (else (test (- x 1))))))

        (test 900)
        "#,
        "done",
    );
}
