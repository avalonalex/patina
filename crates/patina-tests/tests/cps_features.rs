//! CPS-specific feature tests
//!
//! Tests for features that specifically rely on CPS (Continuation-Passing Style) evaluation:
//! - Exception handler stack management
//! - Dynamic-wind with multiple nested levels
//! - Continuation capture edge cases
//! - Prompt tag discrimination (delimited continuations)
//!
//! These tests ensure the CPS evaluator correctly handles complex control flow.

mod common;
use common::*;

// =============================================================================
// Exception Handler Stack Management
// =============================================================================

#[test]
fn test_exception_handler_single() {
    // Single handler catches exception
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (k)
            (with-exception-handler
              (lambda (e) (k (list 'caught e)))
              (lambda () (raise 'my-error)))))
        "#,
        "(caught my-error)",
    );
}

#[test]
fn test_exception_handler_returns_normally() {
    // Handler installed but no exception raised
    assert_program_eval_to(
        r#"
        (with-exception-handler
          (lambda (e) 'never-called)
          (lambda () (+ 1 2 3)))
        "#,
        "6",
    );
}

#[test]
fn test_exception_handler_stack_multiple() {
    // Multiple handlers - innermost should catch
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (escape)
            (with-exception-handler
              (lambda (e) (escape 'outer))
              (lambda ()
                (with-exception-handler
                  (lambda (e) (escape 'inner))
                  (lambda () (raise 'boom)))))))
        "#,
        "inner",
    );
}

#[test]
fn test_exception_handler_pop_after_thunk() {
    // Handler should be popped after thunk completes normally
    // Second raise should go to outer handler
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (escape)
            (with-exception-handler
              (lambda (e) (escape (list 'outer e)))
              (lambda ()
                (with-exception-handler
                  (lambda (e) (escape (list 'inner e)))
                  (lambda () 'ok))
                ; Inner handler is gone now
                (raise 'after-inner)))))
        "#,
        "(outer after-inner)",
    );
}

#[test]
fn test_raise_continuable_handler_return() {
    // raise-continuable allows handler to return a replacement value
    assert_program_eval_to(
        r#"
        (with-exception-handler
          (lambda (e) (* e 10))
          (lambda ()
            (+ 1 (raise-continuable 5))))
        "#,
        "51",
    );
}

#[test]
#[ignore] // BUG: Handler is popped after first raise-continuable, second raise is unhandled
fn test_raise_continuable_multiple_times() {
    // Multiple raise-continuable in sequence
    // This test reveals that the exception handler is consumed after the first
    // raise-continuable, so subsequent raises are unhandled.
    assert_program_eval_to(
        r#"
        (with-exception-handler
          (lambda (e) (+ e 100))
          (lambda ()
            (+ (raise-continuable 1)
               (raise-continuable 2)
               (raise-continuable 3))))
        "#,
        "306",
    );
}

// =============================================================================
// Dynamic-Wind with Multiple Nested Levels
// =============================================================================

#[test]
fn test_dynamic_wind_simple() {
    // Basic dynamic-wind: before, body, after all run
    assert_program_eval_to(
        r#"
        (let ((log '()))
          (dynamic-wind
            (lambda () (set! log (cons 'before log)))
            (lambda () (set! log (cons 'body log)) 'result)
            (lambda () (set! log (cons 'after log))))
          (reverse log))
        "#,
        "(before body after)",
    );
}

#[test]
fn test_dynamic_wind_nested_two_levels() {
    // Two levels of dynamic-wind, verify execution order
    assert_program_eval_to(
        r#"
        (let ((log '()))
          (dynamic-wind
            (lambda () (set! log (cons 'outer-before log)))
            (lambda ()
              (dynamic-wind
                (lambda () (set! log (cons 'inner-before log)))
                (lambda () (set! log (cons 'body log)) 'result)
                (lambda () (set! log (cons 'inner-after log)))))
            (lambda () (set! log (cons 'outer-after log))))
          (reverse log))
        "#,
        "(outer-before inner-before body inner-after outer-after)",
    );
}

#[test]
fn test_dynamic_wind_nested_three_levels() {
    // Three levels of dynamic-wind
    assert_program_eval_to(
        r#"
        (let ((log '()))
          (dynamic-wind
            (lambda () (set! log (cons 'a-in log)))
            (lambda ()
              (dynamic-wind
                (lambda () (set! log (cons 'b-in log)))
                (lambda ()
                  (dynamic-wind
                    (lambda () (set! log (cons 'c-in log)))
                    (lambda () (set! log (cons 'body log)) 'result)
                    (lambda () (set! log (cons 'c-out log)))))
                (lambda () (set! log (cons 'b-out log)))))
            (lambda () (set! log (cons 'a-out log))))
          (reverse log))
        "#,
        "(a-in b-in c-in body c-out b-out a-out)",
    );
}

#[test]
#[ignore] // BUG: After thunk not run when exception is raised in body
fn test_dynamic_wind_with_exception() {
    // Exception raised inside dynamic-wind still runs after thunk
    // This test reveals that when an exception is raised inside dynamic-wind body,
    // the after thunk is NOT executed before the exception propagates.
    // R7RS requires after thunk to run even on non-local exit.
    assert_program_eval_to(
        r#"
        (let ((log '()))
          (guard (e (else (reverse log)))
            (dynamic-wind
              (lambda () (set! log (cons 'before log)))
              (lambda ()
                (set! log (cons 'body log))
                (raise 'boom))
              (lambda () (set! log (cons 'after log))))))
        "#,
        "(before body after)",
    );
}

#[test]
#[ignore] // BUG: After thunk not run when escaping via call/cc
fn test_dynamic_wind_callcc_exit() {
    // Exiting via call/cc should run after thunks
    // This test reveals that the after thunk is NOT executed when
    // escaping a dynamic-wind via call/cc.
    // R7RS requires after thunk to run on continuation escape.
    assert_program_eval_to(
        r#"
        (let ((log '()))
          (call-with-current-continuation
            (lambda (escape)
              (dynamic-wind
                (lambda () (set! log (cons 'before log)))
                (lambda ()
                  (set! log (cons 'body log))
                  (escape 'escaped))
                (lambda () (set! log (cons 'after log))))))
          (reverse log))
        "#,
        "(before body after)",
    );
}

#[test]
fn test_dynamic_wind_callcc_reentry() {
    // Re-entering via call/cc should run before thunks again
    assert_program_eval_to(
        r#"
        (let ((log '())
              (k #f))
          (dynamic-wind
            (lambda () (set! log (cons 'in log)))
            (lambda ()
              (call-with-current-continuation
                (lambda (c) (set! k c)))
              (set! log (cons 'body log)))
            (lambda () (set! log (cons 'out log))))
          (if (< (length log) 6)
              (k #f)
              (reverse log)))
        "#,
        "(in body out in body out)",
    );
}

// =============================================================================
// Continuation Capture Edge Cases
// =============================================================================

#[test]
fn test_callcc_simple_escape() {
    // Basic call/cc escape
    assert_program_eval_to(
        r#"
        (+ 1 (call-with-current-continuation
               (lambda (k) (+ 2 (k 10)))))
        "#,
        "11",
    );
}

#[test]
fn test_callcc_no_escape() {
    // call/cc where continuation is not invoked
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (k) (+ 1 2 3)))
        "#,
        "6",
    );
}

#[test]
fn test_callcc_stored_continuation() {
    // Store continuation and invoke later
    // First iteration: saved gets continuation, returns 10, result=11, 11<20 so call saved(50)
    // Second iteration: continuation returns 50, result=51, 51>=20 so return 51
    assert_program_eval_to(
        r#"
        (let ((saved #f))
          (let ((result
                 (+ 1 (call-with-current-continuation
                        (lambda (k)
                          (set! saved k)
                          10)))))
            (if (< result 20)
                (saved 50)
                result)))
        "#,
        "51",
    );
}

#[test]
fn test_callcc_multiple_invocations() {
    // Invoke same continuation multiple times
    assert_program_eval_to(
        r#"
        (let ((k #f)
              (count 0))
          (+ 1 (call-with-current-continuation
                 (lambda (c)
                   (set! k c)
                   0)))
          (set! count (+ count 1))
          (if (< count 3)
              (k count)
              count))
        "#,
        "3",
    );
}

#[test]
fn test_callcc_nested() {
    // Nested call/cc
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (outer)
            (+ 1 (call-with-current-continuation
                   (lambda (inner)
                     (outer (inner 10)))))))
        "#,
        "11",
    );
}

#[test]
fn test_callcc_in_tail_position() {
    // call/cc in tail position
    assert_program_eval_to(
        r#"
        (define (test)
          (call-with-current-continuation
            (lambda (k) (k 42))))
        (test)
        "#,
        "42",
    );
}

#[test]
fn test_callcc_with_values() {
    // call/cc with multiple values (continuation accepts single value)
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (k)
            (k (values 1 2 3))))
        "#,
        "1\n2\n3",
    );
}

// =============================================================================
// Guard Macro Edge Cases
// =============================================================================

#[test]
fn test_guard_multiple_clauses() {
    // Multiple guard clauses with conditions
    assert_program_eval_to(
        r#"
        (guard (e
                ((number? e) (list 'number e))
                ((symbol? e) (list 'symbol e))
                (else (list 'other e)))
          (raise 42))
        "#,
        "(number 42)",
    );
}

#[test]
fn test_guard_clause_with_expression() {
    // Guard clause condition can be any expression
    assert_program_eval_to(
        r#"
        (guard (e
                ((and (list? e) (= (length e) 2)) (cadr e))
                (else 'no-match))
          (raise (list 'data 100)))
        "#,
        "100",
    );
}

#[test]
fn test_guard_reraise() {
    // Guard with no matching clause re-raises to outer handler
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (escape)
            (with-exception-handler
              (lambda (e) (escape (list 'outer e)))
              (lambda ()
                (guard (inner-e
                        ((number? inner-e) 'number))
                  (raise 'symbol))))))
        "#,
        "(outer symbol)",
    );
}

#[test]
fn test_guard_body_multiple_expressions() {
    // Guard body can have multiple expressions
    assert_program_eval_to(
        r#"
        (guard (e (else 'error))
          (define x 10)
          (define y 20)
          (+ x y))
        "#,
        "30",
    );
}

// =============================================================================
// Exception Handlers with Dynamic-Wind Interaction
// =============================================================================

#[test]
fn test_exception_in_dynamic_wind_before() {
    // Exception in before thunk
    assert_program_eval_to(
        r#"
        (guard (e (else (list 'caught e)))
          (dynamic-wind
            (lambda () (raise 'before-error))
            (lambda () 'body)
            (lambda () 'after)))
        "#,
        "(caught before-error)",
    );
}

#[test]
#[ignore] // BUG: After thunk not run when exception propagates (same as test_dynamic_wind_with_exception)
fn test_exception_in_dynamic_wind_body_after_runs() {
    // Exception in body still runs after thunk
    // This test reveals the same bug as test_dynamic_wind_with_exception
    assert_program_eval_to(
        r#"
        (let ((log '()))
          (guard (e (else (cons 'caught (reverse log))))
            (dynamic-wind
              (lambda () (set! log (cons 'before log)))
              (lambda () (raise 'body-error))
              (lambda () (set! log (cons 'after log))))))
        "#,
        "(caught before after)",
    );
}

#[test]
#[ignore] // BUG: After thunk not run when escaping via call/cc (same as test_dynamic_wind_callcc_exit)
fn test_callcc_escape_runs_dynamic_wind_after() {
    // Escaping via call/cc runs after thunks
    // See test_dynamic_wind_callcc_exit for details
    assert_program_eval_to(
        r#"
        (let ((log '()))
          (call-with-current-continuation
            (lambda (escape)
              (dynamic-wind
                (lambda () (set! log (cons 'before log)))
                (lambda () (escape (reverse log)))
                (lambda () (set! log (cons 'after log)))))))
        "#,
        "(before after)",
    );
}

// =============================================================================
// Error Object Tests
// =============================================================================

#[test]
fn test_error_object_creation() {
    // Create error object with message
    assert_program_eval_to(
        r#"
        (guard (e
                ((error-object? e) (error-object-message e))
                (else 'not-error))
          (error "test message"))
        "#,
        "\"test message\"",
    );
}

#[test]
fn test_error_object_with_irritants() {
    // Error object with irritants
    assert_program_eval_to(
        r#"
        (guard (e
                ((error-object? e) (error-object-irritants e))
                (else 'not-error))
          (error "msg" 'a 'b 'c))
        "#,
        "(a b c)",
    );
}

#[test]
fn test_error_object_predicate_false() {
    // error-object? returns #f for non-error exceptions
    assert_program_eval_to(
        r#"
        (guard (e
                ((error-object? e) 'is-error)
                (else 'not-error))
          (raise 'plain-symbol))
        "#,
        "not-error",
    );
}

// =============================================================================
// Complex Control Flow Combinations
// =============================================================================

#[test]
#[ignore] // BUG: Before/after thunks not run on continuation re-entry
fn test_callcc_across_dynamic_wind_boundary() {
    // Continuation captured inside dynamic-wind, invoked outside
    // This test reveals that when re-entering a continuation that was
    // captured inside dynamic-wind, the before/after thunks are not run.
    // R7RS requires them to run on every entry/exit.
    assert_program_eval_to(
        r#"
        (let ((k #f)
              (log '()))
          (dynamic-wind
            (lambda () (set! log (cons 'in log)))
            (lambda ()
              (call-with-current-continuation
                (lambda (c) (set! k c) 'first)))
            (lambda () (set! log (cons 'out log))))
          (if (eq? (car log) 'in)
              (k 'second)
              (cons 'done (reverse log))))
        "#,
        "(done in out in out)",
    );
}

#[test]
#[ignore] // BUG: Complex interaction between continuations and exception handlers
fn test_exception_with_stored_continuation() {
    // Exception handler uses stored continuation
    // This test reveals issues with scope/binding when combining
    // stored continuations with exception handlers.
    assert_program_eval_to(
        r#"
        (let ((k #f))
          (call-with-current-continuation
            (lambda (escape)
              (with-exception-handler
                (lambda (e)
                  (if k
                      (k (list 'resumed e))
                      (escape (list 'first e))))
                (lambda ()
                  (call-with-current-continuation
                    (lambda (c) (set! k c)))
                  (raise 'error))))))
        "#,
        "(first error)",
    );
}
