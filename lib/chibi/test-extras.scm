;; test-extras.scm -- Test framework macros for (chibi test)
;;
;; This file provides the 'test' macro and helper functions for the test framework.
;; The primitive procedures (test-start, test-end, etc.) are implemented in Rust.
;;
;; Part of (chibi test) library.

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Helper for approximate equality (used by test framework)
;; Matches chibi-scheme's behavior: uses epsilon-based comparison for floats
(define (test-equal? expected actual)
  (define epsilon 1e-6)

  (define (approx-equal? x y)
    (cond
      ;; For real numbers (but not exact rationals), use epsilon-based comparison
      ((and (real? x) (real? y) (not (rational? x)) (not (rational? y)))
       (< (abs (- x y)) epsilon))
      ;; For complex numbers, compare both parts with epsilon
      ((and (complex? x) (complex? y))
       (and (< (abs (- (real-part x) (real-part y))) epsilon)
            (< (abs (- (imag-part x) (imag-part y))) epsilon)))
      ;; For pairs, recurse on both parts
      ((and (pair? x) (pair? y))
       (and (approx-equal? (car x) (car y))
            (approx-equal? (cdr x) (cdr y))))
      ;; For vectors, recurse on all elements
      ((and (vector? x) (vector? y) (= (vector-length x) (vector-length y)))
       (let loop ((i 0))
         (or (>= i (vector-length x))
             (and (approx-equal? (vector-ref x i) (vector-ref y i))
                  (loop (+ i 1))))))
      ;; For everything else, use equal?
      (else (equal? x y))))

  (approx-equal? expected actual))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Test macro
;;
;; Usage:
;;   (test expected-value actual-expression)
;;   (test "test description" expected-value actual-expression)
;;
;; The macro prints what it's about to test BEFORE evaluating the expression,
;; so if the test crashes, you can see which test was running.
;;
;; Note: This macro does NOT catch errors during test execution.
;; If expr raises an error, the interpreter's resilient mode will
;; catch it and continue with the next top-level expression.
;; This allows test suites to continue running even when individual
;; tests encounter unimplemented features or runtime errors.

(define-syntax test
  (syntax-rules ()
    ;; Test with explicit name/description (3 arguments)
    ((test name expected expr)
     (begin
       ;; Print what we're about to test BEFORE evaluating
       (display "Testing: ")
       (write name)
       (display " -> ")
       (write 'expr)
       (newline)
       (let ((expected-val expected))
         ;; Now evaluate the expression
         (let ((actual-val expr))
           ;; Then compare and report using approximate equality
           (if (test-equal? expected-val actual-val)
               (begin
                 (test-increment-passed)
                 #t)  ; Test passed (silently)
               (begin
                 (test-increment-failed)
                 (display "FAIL: ")
                 (write name)
                 (newline)
                 (display "  expr: ")
                 (write 'expr)
                 (newline)
                 (display "  expected: ")
                 (write expected-val)
                 (newline)
                 (display "  but got:  ")
                 (write actual-val)
                 (newline)
                 (newline)
                 #f))))))

    ;; Test without name (2 arguments, backward compatible)
    ((test expected expr)
     (begin
       ;; Print what we're about to test BEFORE evaluating
       (display "Testing: ")
       (write 'expr)
       (newline)
       (let ((expected-val expected))
         ;; Now evaluate the expression
         (let ((actual-val expr))
           ;; Then compare and report using approximate equality
           (if (test-equal? expected-val actual-val)
               (begin
                 (test-increment-passed)
                 #t)  ; Test passed (silently)
               (begin
                 (test-increment-failed)
                 (display "FAIL: ")
                 (write 'expr)
                 (newline)
                 (display "  expected: ")
                 (write expected-val)
                 (newline)
                 (display "  but got:  ")
                 (write actual-val)
                 (newline)
                 (newline)
                 #f)))))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; End of (chibi test) extras
