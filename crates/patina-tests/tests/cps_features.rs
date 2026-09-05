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
fn test_dynamic_wind_callcc_exit() {
    // Exiting via call/cc should run after thunks
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
fn test_callcc_escape_runs_dynamic_wind_after() {
    // (escape (reverse log)) evaluates (reverse log) BEFORE the escape triggers
    // the after-thunk, so the returned value is (before) not (before after).
    // The after-thunk still runs (mutating log), but the escape value is already computed.
    // Verified against chibi-scheme: returns (before).
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
        "(before)",
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
fn test_callcc_across_dynamic_wind_boundary() {
    // Original test had wrong expectation: (eq? (car log) 'in) is always false
    // after normal dynamic-wind exit (log = (out in), car = 'out), so re-entry
    // never happened. Verified against chibi-scheme: returns (done in out).
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
        "(done in out)",
    );
}

#[test]
fn test_callcc_reentry_replays_wind_thunks() {
    // Re-entering a continuation captured inside dynamic-wind must replay
    // before-thunks. Verified against chibi-scheme: returns 3.
    assert_program_eval_to(
        r#"
        (let ((k #f)
              (count 0))
          (dynamic-wind
            (lambda () (set! count (+ count 1)))
            (lambda ()
              (call-with-current-continuation
                (lambda (c) (set! k c))))
            (lambda () #f))
          (if (< count 3)
              (k #f)
              count))
        "#,
        "3",
    );
}

#[test]
fn test_exception_with_stored_continuation() {
    // Exception handler uses escape continuation on first raise.
    // Original test was an infinite loop (verified against chibi-scheme: hangs).
    // Replaced with a terminating test of the same pattern.
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (escape)
            (with-exception-handler
              (lambda (e) (escape (list 'caught e)))
              (lambda () (raise 'error)))))
        "#,
        "(caught error)",
    );
}

// =============================================================================
// Runtime Error Routing Through Exception Handlers
// =============================================================================
// These tests verify that runtime errors (type errors, arity errors, etc.)
// are properly routed through exception handlers when handlers are installed.

#[test]
fn test_guard_catches_type_error() {
    // Type error should be caught by guard
    assert_program_eval_to(
        r#"
        (guard (ex ((error-object? ex) 'caught-type-error))
          (+ "not-a-number" 1))
        "#,
        "caught-type-error",
    );
}

#[test]
fn test_guard_catches_undefined_variable() {
    // Undefined variable should be caught by guard
    assert_program_eval_to(
        r#"
        (guard (ex ((error-object? ex) 'caught-undefined))
          undefined-variable-xyz)
        "#,
        "caught-undefined",
    );
}

#[test]
fn test_guard_catches_arity_error() {
    // Arity error should be caught by guard
    assert_program_eval_to(
        r#"
        (guard (ex ((error-object? ex) 'caught-arity))
          (car 1 2 3))
        "#,
        "caught-arity",
    );
}

#[test]
fn test_guard_catches_division_by_zero() {
    // Division by zero should be caught by guard
    assert_program_eval_to(
        r#"
        (guard (ex ((error-object? ex) 'caught-division))
          (/ 1 0))
        "#,
        "caught-division",
    );
}

#[test]
fn test_guard_catches_bounds_error() {
    // Index out of bounds should be caught by guard
    assert_program_eval_to(
        r#"
        (guard (ex ((error-object? ex) 'caught-bounds))
          (vector-ref (vector 1 2 3) 100))
        "#,
        "caught-bounds",
    );
}

#[test]
fn test_guard_catches_application_error() {
    // Application of non-procedure should be caught by guard
    assert_program_eval_to(
        r#"
        (guard (ex ((error-object? ex) 'caught-app))
          (42 1 2))
        "#,
        "caught-app",
    );
}

#[test]
fn test_with_exception_handler_catches_type_error() {
    // Type error should be caught by with-exception-handler
    assert_program_eval_to(
        r#"
        (call-with-current-continuation
          (lambda (escape)
            (with-exception-handler
              (lambda (ex) (escape (if (error-object? ex) 'caught 'not-error)))
              (lambda () (car "not-a-pair")))))
        "#,
        "caught",
    );
}

#[test]
fn test_error_message_preserved() {
    // The error message should be preserved in the exception object
    assert_program_eval_to(
        r#"
        (guard (ex ((error-object? ex) (error-object-message ex)))
          (car 'not-a-pair))
        "#,
        "\"car expects a pair\"",
    );
}

#[test]
fn test_guard_catches_file_error() {
    // file-error? predicate should work for file-related errors
    assert_program_eval_to(
        r#"
        (import (scheme file))
        (guard (ex ((file-error? ex) 'file-error)
                   ((error-object? ex) 'other-error))
          (open-input-file "/nonexistent/path/that/does/not/exist"))
        "#,
        "file-error",
    );
}

#[test]
fn test_guard_catches_read_error() {
    // read-error? predicate should work for parsing errors
    // Use "1.2.3" which is an invalid number format
    assert_program_eval_to(
        r#"
        (import (scheme read))
        (guard (ex ((read-error? ex) 'read-error)
                   ((error-object? ex) 'other-error))
          (read (open-input-string "1.2.3")))
        "#,
        "read-error",
    );
}

#[test]
fn test_error_object_irritants() {
    // error-object-irritants should return the irritants list
    assert_program_eval_to(
        r#"
        (guard (ex ((error-object? ex) (error-object-irritants ex)))
          (error "test error" 1 2 3))
        "#,
        "(1 2 3)",
    );
}

// =============================================================================
// Backtracking / amb-style continuations (from Chibi test08-callcc.scm)
// =============================================================================

#[test]
fn test_backtracking_pythagorean_triple() {
    // Classic amb-style backtracking using call/cc
    // Finds first Pythagorean triple where x,y,z are in range 2-9
    // Result encodes x*100 + y*10 + z
    // Both 534 (x=5,y=3,z=4) and 543 (x=5,y=4,z=3) are valid
    // Patina finds 534 due to let binding evaluation order
    let code = r#"
        (define fail
          (lambda () 999999))

        (define in-range
          (lambda (a b)
            (call-with-current-continuation
              (lambda (cont)
                (enumerate a b cont)))))

        (define enumerate
          (lambda (a b cont)
            (if (< b a)
                (fail)
                (let ((save fail))
                  (begin
                    (set! fail
                      (lambda ()
                        (begin
                          (set! fail save)
                          (enumerate (+ a 1) b cont))))
                    (cont a))))))

        (let ((x (in-range 2 9))
              (y (in-range 2 9))
              (z (in-range 2 9)))
          (if (= (* x x)
                 (+ (* y y) (* z z)))
              (+ (* x 100) (+ (* y 10) z))
              (fail)))
        "#;

    let result = eval_program(code);
    // Accept either 534 or 543 - both are valid Pythagorean triples
    let value: i64 = result.parse().expect("expected integer result");
    assert!(
        value == 534 || value == 543,
        "Expected 534 (5,3,4) or 543 (5,4,3), got {}",
        value
    );
    // Verify it's actually a Pythagorean triple
    let x = value / 100;
    let y = (value / 10) % 10;
    let z = value % 10;
    assert_eq!(x * x, y * y + z * z, "Not a valid Pythagorean triple");
}

// =============================================================================
// Instruction-level control ops: call-with-values + call/cc
//
// The two *multi-value* continuation cases lived in `backend_divergence.rs`
// while they were a known divergence; both converged on 2026-08-25 and are
// plain both-backend assertions there now, keeping the SRFI 1 abort shape
// beside them. PRD/bugs/TREE_WALKER_CALLCC_MULTI_VALUES.md is closed.
//
// The single-value case below is *not* affected by that bug — it was swept into
// the old `#[cfg(feature = "vm-backend")]` gate with its two neighbours, and
// `assert_divergence` caught it the first time the quarantine was made to prove
// the tree-walker actually fails.
// =============================================================================

#[test]
fn test_callcc_single_value_through_call_with_values() {
    assert_program_eval_to(
        r#"
        (call-with-values
          (lambda ()
            (call-with-current-continuation
              (lambda (k) (k 42))))
          (lambda (x) x))
        "#,
        "42",
    );
}

// =============================================================================
// Instruction-level control ops: dynamic-wind + call/cc re-entry
// =============================================================================

#[test]
fn test_dynamic_wind_callcc_reentry_delivers_value() {
    // Continuation captured inside dynamic-wind body, re-invoked with new value
    assert_program_eval_to(
        r#"
        (let ((k #f)
              (results '()))
          (let ((val
                 (dynamic-wind
                   (lambda () #f)
                   (lambda ()
                     (call-with-current-continuation
                       (lambda (c) (set! k c) 'first)))
                   (lambda () #f))))
            (set! results (cons val results))
            (when (and k (< (length results) 3))
              (let ((saved k))
                (set! k #f)
                (saved 'second))))
          (reverse results))
        "#,
        "(first second)",
    );
}

#[test]
fn test_dynamic_wind_callcc_before_after_run_on_reentry() {
    // Before/after thunks run on each entry/exit including re-entry
    assert_program_eval_to(
        r#"
        (let ((k #f)
              (log '()))
          (dynamic-wind
            (lambda () (set! log (cons 'in log)))
            (lambda ()
              (if (not k)
                  (call-with-current-continuation
                    (lambda (c) (set! k c))))
              'ok)
            (lambda () (set! log (cons 'out log))))
          (if (< (length log) 6)
              (k 'again))
          (reverse log))
        "#,
        "(in out in out in out)",
    );
}

#[test]
fn test_dynamic_wind_callcc_escape_runs_after() {
    // Escaping from dynamic-wind body via call/cc runs after-thunk
    assert_program_eval_to(
        r#"
        (let ((log '()))
          (call-with-current-continuation
            (lambda (escape)
              (dynamic-wind
                (lambda () (set! log (cons 'before log)))
                (lambda ()
                  (set! log (cons 'body log))
                  (escape 'done))
                (lambda () (set! log (cons 'after log))))))
          (reverse log))
        "#,
        "(before body after)",
    );
}

/// Aborting to a prompt out of each of the value form's three thunks —
/// measured against Racket.
///
/// On both backends since 2026-09-04, when the tree-walker's prompt API
/// landed (issue #169); VM-only before that, the two names being exported
/// from `(scheme base)` but implemented on the VM alone. Nothing in the
/// suite covered the *value* form under a prompt at all before 2026-09-02,
/// which is how three defects survived:
///
/// ```text
///                            this VM                      main (7e696892)
///   body, tail position      ((handler ab) (in b1 out))   same
///   body, non-tail           ((handler ab) (in b1 out))   panic: "no active frame"
///   before thunk             ((handler ab) (in))          (#<unspecified> (in body out))
///   after thunk              ((handler ab) (in body out)) (#<unspecified> (in body out))
/// ```
///
/// Racket gives this VM's column for all four (same programs, its
/// one-argument handler). Note the body case only fails on `main` when the
/// `dynamic-wind` is *not* in tail position of the prompt body — the tail
/// shape tail-pops the frame first and happens to survive — so the wrapped
/// spelling below is the one that carries the signal. The other two `main`
/// rows silently skipped the handler *and* ran thunks the abort should have
/// prevented: `body` and `out` after an abort from `before`, which had not
/// entered the extent yet.
///
/// All three follow from the same change as #157/#159: the abort truncates to
/// the prompt's frame depth, and the value form's bookkeeping is now frames
/// rather than a Rust call the truncation walked out from under.
#[test]
fn test_abort_to_a_prompt_out_of_each_value_form_wind_thunk() {
    const PRELUDE: &str = r#"
        (define dw dynamic-wind)
        (define t (make-continuation-prompt-tag 'p))
        (define (probe run)
          (let ((log '()))
            ;; `let*`, not `(list (run …) (reverse log))`: argument order is
            ;; unspecified, and reading the log first is a different program.
            (let* ((r (run (lambda (x) (set! log (cons x log)))))
                   (l (reverse log)))
              (list r l))))
    "#;
    let probe = |body: &str| {
        eval_program(&format!(
            "{PRELUDE}
             (probe (lambda (note)
               (call-with-continuation-prompt
                 (lambda () {body})
                 t
                 (lambda (v k) (list 'handler v)))))"
        ))
    };
    // From the body: the extent was entered, so `out` still runs. Both
    // positions, because only the non-tail one caught the `main` panic.
    assert_eq!(
        probe(
            "(dw (lambda () (note 'in))
                 (lambda () (note 'b1) (abort-current-continuation t 'ab) (note 'b2))
                 (lambda () (note 'out)))"
        ),
        "((handler ab) (in b1 out))",
        "body abort, dynamic-wind in tail position of the prompt body"
    );
    assert_eq!(
        probe(
            "(list 'body-result
                   (dw (lambda () (note 'in))
                       (lambda () (note 'b1) (abort-current-continuation t 'ab) (note 'b2))
                       (lambda () (note 'out))))"
        ),
        "((handler ab) (in b1 out))",
        "body abort, non-tail — this is the shape that panicked on main"
    );
    // From the before-thunk: the extent is not entered until it returns, so
    // neither the body nor the after-thunk runs.
    assert_eq!(
        probe(
            "(dw (lambda () (note 'in) (abort-current-continuation t 'ab))
                 (lambda () (note 'body))
                 (lambda () (note 'out)))"
        ),
        "((handler ab) (in))"
    );
    // From the after-thunk: everything has already run.
    assert_eq!(
        probe(
            "(dw (lambda () (note 'in))
                 (lambda () (note 'body))
                 (lambda () (note 'out) (abort-current-continuation t 'ab)))"
        ),
        "((handler ab) (in body out))"
    );
}

/// Invoking the composable continuation a prompt handler receives —
/// measured against Racket, on both backends since 2026-09-04.
///
/// Its neighbour above covers the *abort* half of this API and deliberately
/// never resumes `k`, which is how issue #160 survived. Appending the
/// captured frames was the whole of the invoke: the value went into a
/// register named by one frame's numbering and indexed against another's
/// base, and what the resumed computation returned went to the register the
/// prompt had been going to write — in a frame the abort had already
/// unwound. Neither delivery reached anybody.
///
/// ```text
///                                    this VM             main (4624b0e8)
///   (list 'got ␣) resumed with 10    (got 10)            ()
///   (+ 1 ␣) resumed with 10          11                  Type error: +
///   k invoked twice                  ((got 1) (got 2))   (() ())
///   an extent inside the capture     re-entered          re-entered
/// ```
///
/// Racket gives this VM's column for every row here. Its spelling differs —
/// an abort there passes only values, so the continuation is captured with
/// `call-with-composable-continuation` and carried through the abort as one —
/// but the programs are otherwise these.
///
/// The two halves of the fix are each invisible without the other, and the
/// first row separates them. Measured, by building each half alone:
/// delivering into the hole while leaving the captured return where it was
/// resumes the computation correctly and then throws its `(got 10)` away —
/// `main`'s answer exactly. Re-pointing the return without delivering gives
/// `(handler ab resumed-to (got ()))`.
#[test]
fn test_a_prompt_handlers_composable_continuation_resumes_its_computation() {
    let probe = |body: &str, handler: &str| {
        eval_program(&format!(
            "(define t (make-continuation-prompt-tag 'p))
             (call-with-continuation-prompt (lambda () {body}) t (lambda (v k) {handler}))"
        ))
    };
    // The issue's shape: `(k 10)` resumes `(list 'got ␣)` with the 10 in the
    // hole, and its own value is that list rather than the empty one.
    assert_eq!(
        probe(
            "(list 'got (abort-current-continuation t 'ab))",
            "(list 'handler v 'resumed-to (k 10))"
        ),
        "(handler ab resumed-to (got 10))"
    );
    // The delivered value is read, not merely dropped: this used to reach `+`
    // with the `NULL` an untouched register holds.
    assert_eq!(
        probe(
            "(+ 1 (abort-current-continuation t 'ab))",
            "(list v (k 10))"
        ),
        "(ab 11)"
    );
    // Composable, so invoking it twice runs the captured computation twice.
    assert_eq!(
        probe(
            "(list 'got (abort-current-continuation t 'ab))",
            "(list (k 1) (k 2))"
        ),
        "((got 1) (got 2))"
    );
    // More than one frame between the prompt and the abort — the relocation
    // has to keep the appended frames' return chain pointing at each other,
    // and only its outermost frame at the invoke.
    assert_eq!(
        probe(
            "(list 'a ((lambda (z) (list 'b z (abort-current-continuation t 'ab))) 'zz))",
            "(list 'handler v (k 10))"
        ),
        "(handler ab (a (b zz 10)))"
    );
    // `k` in tail position of the handler: the invoking frame is popped
    // before the append, so the resumed computation returns past it.
    assert_eq!(
        probe("(list 'got (abort-current-continuation t 'ab))", "(k 10)"),
        "(got 10)"
    );
    // Nothing between the abort and its prompt — the abort is in tail
    // position of the body, so that frame is already popped when the capture
    // happens — makes `k` the identity, and `(k 10)` is 10. Both positions,
    // because the tail one returns through the handler's own frame.
    assert_eq!(
        probe("(abort-current-continuation t 'ab)", "(list v (k 10))"),
        "(ab 10)"
    );
    assert_eq!(probe("(abort-current-continuation t 'ab)", "(k 10)"), "10");
    // Reached through `call_any` rather than an instruction. The value form
    // of `call-with-values` runs its producer across a nested Rust boundary
    // that still owes the consumer a call, and reporting the resumed frames
    // as a continuation *escape* told it to abandon that: the two spellings
    // of one program disagreed, `(handler ())` against the head form's
    // answer. A composable continuation returns, so its invoke reports a
    // pushed frame, which is what it is. `(k)` with no arguments delivers a
    // `#<values>` object exactly as `(values)` would.
    let head = probe(
        "(list 'got (abort-current-continuation t 'ab))",
        "(list 'handler (call-with-values k list))",
    );
    let value_form = eval_program(
        "(define t (make-continuation-prompt-tag 'p))
         (define cwv call-with-values)
         (call-with-continuation-prompt
           (lambda () (list 'got (abort-current-continuation t 'ab)))
           t
           (lambda (v k) (list 'handler (cwv k list))))",
    );
    assert_eq!(
        head, value_form,
        "the head and value forms of call-with-values are the same program"
    );
    assert_eq!(head, "(handler ((got #<values>)))");
    // `k` as the exception handler itself, with *two* frames captured: the
    // raise runs the handler down to the depth it started at, which stops
    // being `frames.len() - 1` as soon as an invoke can push more than one
    // frame. Reading it after the call gave `(handler (a ()))` — the resumed
    // computation cut in half.
    assert_eq!(
        probe(
            "(list 'a ((lambda (z) (list 'b z (abort-current-continuation t 'ab))) 'zz))",
            "(list 'handler (with-exception-handler k (lambda () (raise-continuable 'boom))))"
        ),
        "(handler (a (b zz boom)))"
    );
    // A `dynamic-wind` extent inside the captured region is entered again by
    // the invoke — composable invokes do not travel, so this happens whether
    // or not the live stack already shares the extent — and left when the
    // resumed computation returns through it.
    assert_eq!(
        eval_program(
            r#"
            (define t (make-continuation-prompt-tag 'p))
            (define log '())
            (define (note x) (set! log (cons x log)))
            (define r
              (call-with-continuation-prompt
                (lambda ()
                  (dynamic-wind
                    (lambda () (note 'in))
                    (lambda () (list 'got (abort-current-continuation t 'ab)))
                    (lambda () (note 'out))))
                t
                (lambda (v k) (list 'handler v (k 10)))))
            (list r (reverse log))
            "#
        ),
        "((handler ab (got 10)) (in out in out))"
    );
}

/// What an abort leaves behind, and what a composable continuation carries —
/// measured against Racket 9.3 and Gauche 0.9.15, on both backends since
/// 2026-09-04.
///
/// Both are cells of the dynamic-state matrix in `docs/VM_RUNTIME.md` §5.6:
/// of the five components of `VmState` that belong to a dynamic extent rather
/// than to the machine, the abort was unwinding four and the delimited capture
/// was saving three.
///
/// ```text
///                                              this VM          main (62a47df9)
///   abort uninstalls the region's handlers     ESCAPED-TO-TOP   INNER
///   composable k carries its handler stack     (got (INNER …))  unhandled exception
///   composable k carries an inner prompt       (outer (inner…)) no matching prompt tag
/// ```
///
/// The oracles: Racket answers the abort and inner-prompt rows directly, and
/// **Gauche** answers the handler row through `gauche.partcont`'s
/// `shift`/`reset` with real `with-exception-handler` and `raise-continuable`
/// — which matters, because Racket's handlers *are* continuation marks, so
/// Racket alone could not tell "carries the dynamic environment" from an
/// artifact of that representation.
///
/// **Why the boundary is a recorded depth and not a frame depth.** The first
/// version of this fix inferred it, and two of the rows below are the shapes
/// that killed that: a `with-exception-handler` in *tail position of the
/// prompt body* installs at the prompt's own frame depth, because the body
/// frame is already popped — and so does a handler whose thunk *tail-calls*
/// `call-with-continuation-prompt`. One is inside the prompt and must go, the
/// other encloses it and must stay, and they are indistinguishable by depth.
/// `PromptFrame::exception_handler_depth` records the length at push time, as
/// `dynamic_wind_depth` has always done for winds.
#[test]
fn test_a_prompt_transfers_carry_the_dynamic_environment() {
    const T: &str = "(define t (make-continuation-prompt-tag 'p))";

    // The abort leaves the extents it unwinds, so the handler installed inside
    // the prompt body must not catch a raise made from the prompt handler.
    // The enclosing handler here reaches the prompt by a *tail* call, so it
    // shares the prompt's frame depth and must survive anyway.
    assert_eq!(
        eval_program(&format!(
            "{T}
             (with-exception-handler
               (lambda (e) (list 'ESCAPED-TO-TOP e))
               (lambda ()
                 (call-with-continuation-prompt
                   (lambda ()
                     (with-exception-handler
                       (lambda (e) (list 'INNER e))
                       (lambda () (list 'got (abort-current-continuation t 'ab)))))
                   t
                   (lambda (v k) (raise-continuable 'boom)))))"
        )),
        "(ESCAPED-TO-TOP boom)",
        "an abort must uninstall the handlers of the region it abandons"
    );
    // The mirror shape: the enclosing handler is at a depth of its own, and the
    // *inner* one is the one sharing the prompt's depth. Same answer required,
    // for the opposite reason — this is the row that a depth test gets wrong
    // in whichever direction the other row gets right.
    assert_eq!(
        eval_program(&format!(
            "{T}
             (with-exception-handler
               (lambda (e) (list 'TOP e))
               (lambda ()
                 (list 'r
                   (call-with-continuation-prompt
                     (lambda ()
                       (with-exception-handler
                         (lambda (e) (list 'INNER e))
                         (lambda () (list 'got (abort-current-continuation t 'ab)))))
                     t
                     (lambda (v k) (raise-continuable 'boom))))))"
        )),
        "(r (TOP boom))"
    );
    // A composable continuation carries the handlers it was captured under.
    // Escaping it out of the prompt first is what makes this independent of
    // the row above: the handler the abort used to leak has been swept by the
    // ordinary depth sweep before the resume, so a wrong answer here cannot be
    // rescued by a wrong answer there. The two used to mask each other.
    assert_eq!(
        eval_program(&format!(
            "{T}
             (define k* #f)
             (define captured
               (call-with-continuation-prompt
                 (lambda ()
                   (with-exception-handler
                     (lambda (e) (list 'INNER e))
                     (lambda () (list 'got (raise-continuable (abort-current-continuation t 'ab))))))
                 t
                 (lambda (v k) (set! k* k) 'captured)))
             (define resumed (k* 'boom))
             ;; …and they do not outlive the resumed frames: the depth sweep
             ;; that pops any other handler pops these, because their recorded
             ;; depths were relocated with the frames.
             (define after (with-exception-handler
                             (lambda (e) (list 'OUTER e))
                             (lambda () (raise-continuable 'again))))
             (list captured resumed after)"
        )),
        "(captured (got (INNER boom)) (OUTER again))"
    );
    // And it carries a prompt established inside the captured region: the
    // resumed frames abort to a delimiter their own code set up.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 't))
             (define u (make-continuation-prompt-tag 'u))
             (call-with-continuation-prompt
               (lambda ()
                 (list 'outer
                   (call-with-continuation-prompt
                     (lambda () (list 'inner (abort-current-continuation t 'ab)
                                             (abort-current-continuation u 'ab2)))
                     u
                     (lambda (v2 k2) (list 'inner-handler v2)))))
               t
               (lambda (v k) (list 'handler v (k 10))))"
        ),
        "(handler ab (outer (inner-handler ab2)))"
    );
}

/// Resuming a composable continuation somewhere its captured depths do not
/// already fit — measured against Guile 3.0.11, on both backends since
/// 2026-09-04. The tree-walker's carried prompts record two depths rather
/// than three (a CPS continuation is not a stack) and relocate them the same
/// way; these five rows are what hold that to the same answers.
///
/// The test above pins the *semantics*. Every one of its rows happens to
/// resume at wind depth 0 and handler depth 0, with the carried prompt's
/// recorded depths equal to the delimiting prompt's, so every relocation in
/// the invoke cancels to zero and all four rows pass with the arithmetic
/// deleted. These do not.
///
/// A `PromptFrame` records a position in **three** stacks — frames, winds,
/// handlers — and a carried prompt needs all three moved onto the live ones.
/// Relocating only the frame depth gave, in order below: an enclosing
/// `after` thunk run early, handlers enclosing the invoke uninstalled, and a
/// panic. Two more rows cover the abort itself: its handler truncation runs
/// before the after-thunks rather than after (a raise from an after-thunk was
/// still reaching the abandoned handler — #162 in the one window the first fix
/// missed), and a capture whose recorded depth has outrun the live stack
/// clamps instead of slicing (`raise` pops the handler entry it is running
/// before calling it, so a handler aborting to a prompt under itself arrives
/// with a shorter stack than the prompt recorded).
///
/// Guile is the oracle here rather than Racket: `(ice-9 control)`'s
/// `call-with-prompt` / `abort-to-prompt` are tagged like Patina's *and* it
/// has R7RS `with-exception-handler` and `raise-continuable`, so all five
/// programs transcribe one-for-one. It agrees on every row.
#[test]
fn test_a_composable_continuation_relocates_the_depths_it_carries() {
    // Capture a continuation whose region contains an inner prompt, so the
    // carried `PromptFrame` has depths of its own to relocate.
    const CAPTURE: &str = "
        (define t (make-continuation-prompt-tag 't))
        (define u (make-continuation-prompt-tag 'u))
        (define k* #f)
        (define (capture!)
          (call-with-continuation-prompt
            (lambda ()
              (list 'outer
                (call-with-continuation-prompt
                  (lambda () (list 'inner (abort-current-continuation t 'ab)
                                          (abort-current-continuation u 'ab2)))
                  u (lambda (v2 k2) (list 'inner-handler v2)))))
            t (lambda (v k) (set! k* k) 'captured)))";

    // Resumed inside a `dynamic-wind`: the abort to the carried prompt must
    // truncate the live wind stack at *its* extent, not at the depth it
    // recorded against another stack. Unrelocated, `OUT` ran twice.
    assert_eq!(
        eval_program(&format!(
            "{CAPTURE}
             (define log '())
             (capture!)
             (define r (dynamic-wind (lambda () (set! log (cons 'IN log)))
                                     (lambda () (k* 10))
                                     (lambda () (set! log (cons 'OUT log)))))
             (list r (reverse log))"
        )),
        "((outer (inner-handler ab2)) (IN OUT))"
    );
    // Resumed under a handler: the abort to the carried prompt must not
    // uninstall handlers that enclose the invoke. Unrelocated, the raise after
    // it was unhandled.
    assert_eq!(
        eval_program(&format!(
            "{CAPTURE}
             (capture!)
             (with-exception-handler
               (lambda (e) (list 'H e))
               (lambda () (let ((r (k* 10))) (list r (raise-continuable 'ping)))))"
        )),
        "((outer (inner-handler ab2)) (H ping))"
    );
    // Captured inside a `dynamic-wind` and resumed outside every one: the
    // carried prompt records a wind depth the live stack cannot reach, and
    // slicing at it panicked.
    assert_eq!(
        eval_program(&format!(
            "{CAPTURE}
             (define captured
               (dynamic-wind (lambda () #f)
                             (lambda () (capture!))
                             (lambda () #f)))
             (list captured (k* 10))"
        )),
        "(captured (outer (inner-handler ab2)))"
    );
    // A raise from an after-thunk the abort itself runs. The handler installed
    // inside the region being abandoned must already be gone — this is the
    // answer the `call/cc` path gives for the same shape, because a jump runs
    // each thunk under its own record's handler stack.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 'p))
             (define log '())
             (define r
               (with-exception-handler
                 (lambda (e) (set! log (cons (list 'TOP e) log)) 'top)
                 (lambda ()
                   (call-with-continuation-prompt
                     (lambda ()
                       (dynamic-wind
                         (lambda () #f)
                         (lambda ()
                           (with-exception-handler
                             (lambda (e) (set! log (cons (list 'INNER e) log)) 'inner)
                             (lambda () (abort-current-continuation t 'ab))))
                         (lambda () (raise-continuable 'from-after))))
                     t
                     (lambda (v k) (list 'prompt-handler v))))))
             (list r (reverse log))"
        ),
        "((prompt-handler ab) ((TOP from-after)))"
    );
    // A handler aborting to a prompt established inside its own thunk: the
    // raise has already popped the entry it is running, so the live handler
    // stack is shorter than the prompt recorded. Slicing at the recorded
    // length panicked.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 'p))
             (with-exception-handler
               (lambda (e) (abort-current-continuation t (list 'aborted e)))
               (lambda ()
                 (call-with-continuation-prompt
                   (lambda () (raise-continuable 'x))
                   t
                   (lambda (v k) (list 'handler v)))))"
        ),
        "(handler (aborted x))"
    );
}

/// An abort's after-thunks: the dynamic environment they run in, and what
/// happens when a continuation captured inside one is re-entered. Measured
/// against Guile 3.0.11, on both backends since 2026-09-04.
///
/// A jump has run each wind thunk under its own record's handler stack, in a
/// frame of its own, since #156; the value form of `dynamic-wind` since #158.
/// The abort ran its after-thunks on a nested Rust loop instead, and had both
/// of the defects that follow:
///
/// ```text
///                                     this VM              main (7ebfed1f)
///   after-thunk's raise, handler       (MID from-after)     (TOP from-after)
///     captured by the record
///   re-enter an after-thunk            after-1 once,        after-1 twice,
///                                      value kept           value lost
/// ```
///
/// It is a jump now: it travels to a continuation whose top frame calls the
/// prompt handler, because leaving every extent between here and a target is
/// what a travel *is*.
///
/// A composable invoke's re-entry thunks deliberately do **not** go through
/// the travel — the test below pins why.
#[test]
fn test_an_aborts_after_thunks_run_as_frames() {
    // The handler sits *between* the prompt and the `dynamic-wind`, so the
    // record captured it and its after-thunk must run under it. The same
    // program left by a jump instead of an abort answers `MID` on both
    // backends, chibi, Gauche and Guile.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 'p))
             (define log '())
             (define (note x) (set! log (cons x log)))
             (define r
               (with-exception-handler
                 (lambda (e) (note (list 'TOP e)) 'top)
                 (lambda ()
                   (call-with-continuation-prompt
                     (lambda ()
                       (with-exception-handler
                         (lambda (e) (note (list 'MID e)) 'mid)
                         (lambda ()
                           (dynamic-wind
                             (lambda () #f)
                             (lambda () (abort-current-continuation t 'ab))
                             (lambda () (raise-continuable 'from-after))))))
                     t
                     (lambda (v k) (list 'prompt-handler v))))))
             (list r (reverse log))"
        ),
        "((prompt-handler ab) ((MID from-after)))"
    );
    // Re-entering a continuation captured inside an after-thunk the abort is
    // running resumes *after* the capture — `after-1` once — and the abort
    // still delivers its value. On main the thunk restarted from the top and
    // `r` came out `#<unspecified>`: #157's signature, on the abort.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 'p))
             (define k #f) (define n 0) (define log '())
             (define (note x) (set! log (cons x log)))
             (define r
               (call-with-continuation-prompt
                 (lambda ()
                   (dynamic-wind (lambda () (note 'before))
                                 (lambda () (abort-current-continuation t 'ab))
                                 (lambda () (note 'after-1)
                                            (call/cc (lambda (c) (set! k c)))
                                            (note 'after-2))))
                 t (lambda (v k2) (list 'prompt-handler v))))
             (if (< n 1) (begin (set! n 1) (k 'again)))
             (list r (reverse log))"
        ),
        "((prompt-handler ab) (before after-1 after-2 after-2))"
    );
    // Escaping *out* of an after-thunk the abort is running already worked and
    // has to keep working: the travel reports the escape sentinel like any
    // other, rather than swallowing it.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 'p))
             (define esc #f)
             (define r (call/cc (lambda (c) (set! esc c) 'first)))
             (if (eq? r 'first)
                 (call-with-continuation-prompt
                   (lambda ()
                     (dynamic-wind (lambda () #f)
                                   (lambda () (abort-current-continuation t 'ab))
                                   (lambda () (esc 'escaped-from-after))))
                   t (lambda (v k) (list 'prompt-handler v)))
                 #f)
             r"
        ),
        "escaped-from-after"
    );
}

/// A composable invoke's re-entry `before` thunks run under the **invoke
/// site's** handler stack — measured against Guile 3.0.11, on both backends
/// since 2026-09-04. The tree-walker's `resume_composable` runs them under
/// the live stack for the reason given here; `jump_to_continuation` installs
/// the record's.
///
/// This is the row that says why the abort's fix does not generalise. Routing
/// the abort through `step_wind_jump` is right: its target *replaces* the
/// machine, so `install_thunk_handlers` installing the record's own captured
/// stack is exactly R7RS 6.10. A composable invoke's target would *extend* the
/// machine, and there the same call is wrong — the invoke site's handlers
/// disappear and capture-site ones whose extent is long over come back.
///
/// That shipped for one review cycle. Both rows below answer as Guile does,
/// and as they did before it; under the travel the first died with `Error:
/// unhandled exception: boom-in` and the second answered `CAPTURE-SITE`.
///
/// These thunks *are* frames now — issue #167 gave them a
/// `ResumeComposableInvoke` stub of their own — which is a different thing
/// from routing them through the travel, and the reason it is a separate
/// mechanism. This test is what keeps the two apart.
#[test]
fn test_a_re_entry_thunk_runs_under_the_invoke_sites_handlers() {
    // A `guard` around the invoke must see a raise from the re-entered
    // extent's before-thunk.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 'p))
             (define log '())
             (define (note x) (set! log (cons x log)))
             (define k* #f)
             (define cap
               (call-with-continuation-prompt
                 (lambda ()
                   (dynamic-wind (lambda () (note 'in) (if k* (raise 'boom-in)))
                                 (lambda () (list 'got (abort-current-continuation t 'ab)))
                                 (lambda () (note 'out))))
                 t (lambda (v k) (set! k* k) 'cap)))
             (define r (guard (e (#t (note (list 'G e)) 'guarded)) (k* 5)))
             (list cap r (reverse log))"
        ),
        "(cap guarded (in out in (G boom-in)))"
    );
    // And a handler at the invoke site wins over one whose extent ended when
    // the continuation was captured.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 'p))
             (define k* #f) (define seen #f)
             (define cap
               (with-exception-handler
                 (lambda (e) (set! seen 'CAPTURE-SITE) 'c)
                 (lambda ()
                   (call-with-continuation-prompt
                     (lambda ()
                       (dynamic-wind (lambda () (if k* (raise-continuable 'boom-in)))
                                     (lambda () (list 'got (abort-current-continuation t 'ab)))
                                     (lambda () #f)))
                     t (lambda (v k) (set! k* k) 'cap)))))
             (define r (with-exception-handler
                         (lambda (e) (set! seen 'INVOKE-SITE) 'i)
                         (lambda () (k* 5))))
             (list cap r seen)"
        ),
        "(cap (got 5) INVOKE-SITE)"
    );
}

/// Two shapes that only work once a composable invoke's re-entry thunks are
/// frames — measured against Guile 3.0.11, on both backends since 2026-09-04.
///
/// `control_flow_matrix.rs` covers issue #167 in four spellings, all of them
/// one extent re-entered and returned from normally. These are the two shapes
/// the same change fixes that its axes do not reach: a *transfer out of* a
/// re-entry thunk, and *nested* captured extents. Both were wrong before and
/// neither would have been noticed — the matrix lists nested extents as an
/// axis it does not have yet, and a thunk that aborts is not on any axis at
/// all.
///
/// ```text
///                              this VM                    main (a18d7ae1)
///   abort out of a re-entry    (outer esc), (in out in)   swallowed: (got 5),
///     before-thunk                                        (in out in out)
///   nested captured extents,   (got 5), B re-entered      value lost, B never
///     inner thunk jumps out                               re-entered
/// ```
///
/// Guile answers this VM's column for both.
#[test]
fn a_transfer_out_of_a_re_entry_thunk_behaves() {
    // The re-entered extent's `before` thunk aborts to a prompt outside the
    // invoke. The abort must win — the resumed computation never runs, and
    // the extent it was entering is left without running its `after`, because
    // a `before` that does not return never entered.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 't))
             (define t2 (make-continuation-prompt-tag 't2))
             (define k* #f) (define log '())
             (define (note x) (set! log (cons x log)))
             (define cap
               (call-with-continuation-prompt
                 (lambda ()
                   (dynamic-wind
                     (lambda () (note 'in) (if k* (abort-current-continuation t2 'esc) #f))
                     (lambda () (list 'got (abort-current-continuation t 'ab)))
                     (lambda () (note 'out))))
                 t (lambda (v k) (set! k* k) 'cap)))
             (define r (call-with-continuation-prompt
                         (lambda () (k* 5)) t2 (lambda (v k) (list 'outer v))))
             (list cap r (reverse log))"
        ),
        "(cap (outer esc) (in out in))"
    );
    // Two nested captured extents, with the *outer* `before` thunk capturing a
    // continuation that is re-entered later. Resuming it has to finish
    // entering the inner extent too — on `main` the inner one was never
    // re-entered and the resumed value was lost.
    assert_eq!(
        eval_program(
            "(define t (make-continuation-prompt-tag 't))
             (define k* #f) (define kt #f) (define n 0) (define log '())
             (define (note x) (set! log (cons x log)))
             (define cap
               (call-with-continuation-prompt
                 (lambda ()
                   (dynamic-wind
                     (lambda () (note 'A-in1) (call/cc (lambda (c) (set! kt c))) (note 'A-in2))
                     (lambda () (dynamic-wind (lambda () (note 'B-in))
                                              (lambda () (list 'got (abort-current-continuation t 'ab)))
                                              (lambda () (note 'B-out))))
                     (lambda () (note 'A-out))))
                 t (lambda (v k) (set! k* k) 'cap)))
             (define r (k* 5))
             (when (< n 1) (set! n 1) (kt 'again))
             (list cap r (reverse log))"
        ),
        "(cap (got 5) (A-in1 A-in2 B-in B-out A-out A-in1 A-in2 B-in B-out A-out A-in2 B-in B-out A-out))"
    );
}

/// What a raise still owes once its handler returns is a **frame**, so a
/// continuation captured inside the handler carries it — issue #178, both
/// backends, measured against Guile 3.0.11.
///
/// `backend_divergence.rs` holds the four answers that changed when this
/// landed. What is here is the property none of them pins: **when the
/// reinstated handler goes away again.** R7RS 6.11 puts it back for the rest
/// of the thunk's extent, so a replay of that debt has to end where the
/// replayed region does — otherwise a handler answers a raise long after its
/// `with-exception-handler` returned, and which one answers depends on frame
/// counts nobody wrote down.
///
/// Both backends record a boundary for it, by different means: the VM gives
/// the entry a depth measured from the floor of the smallest region that
/// could replay the frame (`vm_raise_value`), and the tree-walker sweeps the
/// handler stack back to the prompt frame's `handler_depth` when the boundary
/// is reached (`cps_eval/continuation.rs`).
///
/// **Finding programs that can see any of this is most of the work**, and the
/// ones that cannot are worth naming, because each looks like it tests the
/// property:
///
/// - `(raise-continuable (abort-current-continuation …))` captures at the
///   *abort*, which runs first — no raise-step frame is in the region at all.
/// - resuming at the depth it was captured at makes every relocation cancel,
///   the trap `test_a_composable_continuation_relocates_the_depths_it_carries`
///   documents for carried prompt depths.
/// - a `guard` around the later raise installs its own handler on top and
///   answers whichever way the one under test went.
/// - invoking the continuation twice and reading only the two values: an
///   implementation that pushed a handler per invoke and popped none gives
///   the identical answer. The trailing raise below is what makes it bite.
#[test]
fn the_remainder_of_a_raise_is_a_frame() {
    /// `k*` resumes a body whose handler aborted, with `nest` frames between
    /// the `with-exception-handler` and the prompt. `INSIDE` puts the handler
    /// within the captured region, `OUTSIDE` puts it beyond the region's
    /// floor — the case whose depth no region contains.
    fn capture(nest: usize, inside: bool) -> String {
        let body = "(call-with-continuation-prompt\n\
             \x20 (lambda () (list 'body (raise-continuable 'rc)))\n\
             \x20 t (lambda (v k) (set! k* k) 'cap))";
        let handler = "(lambda (e) (if (eq? e 'rc) (abort-current-continuation t 'ab) \
                        (list 'INNER e)))";
        let guarded = if inside {
            format!(
                "(call-with-continuation-prompt\n\
                 \x20 (lambda () (with-exception-handler {handler}\n\
                 \x20   (lambda () (list 'body (raise-continuable 'rc)))))\n\
                 \x20 t (lambda (v k) (set! k* k) 'cap))"
            )
        } else {
            format!(
                "(with-exception-handler {handler} (lambda () (nest {nest} (lambda () {body}))))"
            )
        };
        format!(
            "(define t (make-continuation-prompt-tag 'p))\n\
             (define k* #f)\n\
             (define (nest n thunk) (if (= n 0) (thunk) (list 'L (nest (- n 1) thunk))))\n\
             (define cap {guarded})"
        )
    }

    // The handler is installed *inside* the prompt, so the region contains
    // its extent. Resuming replays the re-push; once the resumed body has
    // returned, the next raise must reach OUTER. Nesting the resume puts the
    // replay at a depth the capture never saw.
    assert_program_eval_to(
        &format!(
            "{}\n\
             (with-exception-handler (lambda (e) (list 'OUTER e))\n\
             \x20 (lambda () (nest 6 (lambda ()\n\
             \x20   (let ((r (k* 'resumed))) (list r (raise-continuable 'after)))))))",
            capture(0, true)
        ),
        "(L (L (L (L (L (L ((body resumed) (OUTER after))))))))",
    );

    // The handler is installed *outside* the prompt, so its own depth names a
    // frame below anything the region owns. Reinstating there on a replay put
    // the entry where nothing sweeps it, and the answer flipped to INNER as
    // soon as one frame separated the handler from the prompt — the VM was
    // right at 0 and wrong from 1, the tree-walker wrong at every count.
    // Guile answers OUTER throughout.
    for n in [0usize, 1, 2, 5, 20] {
        assert_program_eval_to(
            &format!(
                "{}\n\
                 (with-exception-handler (lambda (e) (list 'OUTER e))\n\
                 \x20 (lambda () (let ((r (k* 'resumed))) (list r (raise-continuable 'after)))))",
                capture(n, false)
            ),
            "((body resumed) (OUTER after))",
        );
    }

    // Invoking the same continuation twice reinstalls once per resumption and
    // leaves nothing behind: the trailing raise reaches OUTER, which it would
    // not if the two re-pushes had accumulated.
    assert_program_eval_to(
        &format!(
            "{}\n\
             (with-exception-handler (lambda (e) (list 'OUTER e))\n\
             \x20 (lambda () (let* ((a (k* 'one)) (b (k* 'two)))\n\
             \x20   (list a b (raise-continuable 'after)))))",
            capture(0, true)
        ),
        "((body one) (body two) (OUTER after))",
    );
}

/// The value form of `dynamic-wind` costs a VM frame per nesting level, not a
/// Rust one — VM-only, on a deliberately small stack.
///
/// It used to run its body on a nested dispatch loop, so N nested value-form
/// extents meant N nested Rust calls: 5000 of them aborted the process with
/// `fatal runtime error: stack overflow` on `main` (`7e696892`, release,
/// macOS, 8 MB main-thread stack), while 4000 passed. Running the same
/// instructions head position compiles to, in a stub frame, removed the Rust
/// recursion along with the bugs it caused (issue #157): the whole nest is
/// heap-allocated `CallFrame`s now, and 20000 runs fine.
///
/// Three things about the shape of this test:
///
/// - It runs on a thread with an **explicitly sized** stack. A stack overflow
///   aborts the process rather than failing one test, so a default-stack
///   version took all 50 tests in this binary down with a bare `fatal runtime
///   error` naming none of them. `STACK` also makes the margin a property of
///   the test rather than of the build profile and platform it lands in — the
///   4000/5000 figures above were measured in release, and `cargo test` runs
///   at `opt-level = 1` across two CI platforms.
/// - `STACK` is a small multiple of what the fixed VM needs, which is a
///   constant: it passes at 256 KB, and `main` cannot reach 5000 at any size
///   this machine will give a thread.
/// - It is **VM-only**, unlike its neighbours. The tree-walker is a CPS
///   evaluator that does use Rust stack per level, so including it would
///   measure that instead and force `STACK` past 1 MB. Both backends are held
///   to nested value-form winds *semantically* by the test below.
#[test]
fn test_nested_value_form_winds_do_not_nest_rust_frames() {
    const DEPTH: usize = 5000;
    const STACK: usize = 1024 * 1024;
    let handle = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(|| {
            let program = format!(
                r#"
                (define dw dynamic-wind)
                (define (nest n)
                  (if (= n 0)
                      'done
                      (dw (lambda () #f) (lambda () (nest (- n 1))) (lambda () #f))))
                (nest {DEPTH})
                "#
            );
            assert_eq!(eval_program_vm(&program), "done");
        })
        .expect("spawn");
    if let Err(panic) = handle.join() {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn test_reentering_nested_value_form_winds_runs_each_thunk_once() {
    // Re-entering a continuation captured inside two nested extents runs both
    // before-thunks, once each, and leaves the extents standing.
    //
    // The value form of `dynamic-wind` is what made this a real test. Its
    // records used to carry the frame depth of the call, and
    // `pop_resolved_winds` read that depth to decide a body had returned. A
    // jump that *enters* an extent pushes its record back, and the depth on it
    // belongs to a stack that is not the live one — captured deep, re-entered
    // from the top level, it sat above every frame the travel had, so the next
    // before-thunk's return looked like this extent's body returning. The VM
    // ran `out-a` under the still-running entry and went round for ever;
    // head-position `dynamic-wind` could not reach it, because `PushWind`
    // records carried the "not auto-popped" sentinel already.
    //
    // Neither half of that exists now (issue #157, 2026-09-02): the value form
    // runs `PushWind` too, records carry no depth, and `pop_resolved_winds` is
    // gone. The program stays because the shape it exercises — re-entry across
    // two nested extents, from a much shallower stack — is worth holding both
    // backends to whatever the mechanism underneath.
    //
    // `note` raises past a bounded log, so a regression *fails* rather than
    // spinning: the pre-fix VM looped for ever here, and an unbounded version
    // of this test would show up as a `cargo test` run that never terminates
    // and names no failing test — on CI, as a job timeout.
    assert_program_eval_to(
        r#"
        (define k #f)
        (define log '())
        (define (note x)
          (if (> (length log) 12) (error "wind thunks are looping"))
          (set! log (cons x log)))
        (define dw dynamic-wind)
        (define (deep n)
          (if (= n 0)
              (dw (lambda () (note 'in-a))
                  (lambda ()
                    (dw (lambda () (note 'in-b))
                        (lambda () (call/cc (lambda (c) (set! k c) 'first)))
                        (lambda () (note 'out-b))))
                  (lambda () (note 'out-a)))
              (car (list (deep (- n 1))))))
        (define first-time? (eq? 'first (deep 6)))
        (if first-time? (k 'second) #f)
        (reverse log)
        "#,
        "(in-a in-b out-b out-a in-a in-b out-b out-a)",
    );
}
