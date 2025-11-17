;; test-extras.scm -- Test framework macros for (chibi test)
;;
;; This file provides the 'test' macro and helper functions for the test framework.
;; The primitive procedures (test-begin, test-end, etc.) are implemented in Rust.
;;
;; Part of (chibi test) library.

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Helper for approximate equality (used by test framework)
;;
;; Supports:
;; - Approximate comparison for inexact real numbers (using epsilon = 1e-6)
;; - Approximate comparison for inexact complex numbers (compares real and imaginary parts)
;; - Recursive comparison for pairs and vectors
;; - Exact comparison for everything else (exact integers, rationals, strings, booleans, etc.)
(define (test-equal? expected actual)
  (define epsilon 1e-6)

  (define (approx-equal? x y)
    (cond
      ;; For inexact complex numbers (non-real), compare real and imaginary parts separately
      ;; Note: Check (not (real? x)) to avoid infinite recursion, since all numbers are complex
      ((and (complex? x) (complex? y)
            (not (real? x)) (not (real? y))
            (or (inexact? x) (inexact? y)))
       (and (approx-equal? (real-part x) (real-part y))
            (approx-equal? (imag-part x) (imag-part y))))
      ;; For real numbers (but not exact rationals), use epsilon-based comparison
      ((and (real? x) (real? y) (inexact? x) (inexact? y))
       (< (abs (- x y)) epsilon))
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
      ;; For everything else (exact integers, rationals, strings, etc.), use exact equal?
      (else (equal? x y))))

  (approx-equal? expected actual))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Test macro
(define-syntax test
  (syntax-rules ()
    ;; Test without name (2 arguments)
    ((test expected expr)
     (begin
       (display "Testing: ")
       (write 'expr)
       (newline)
       (let ((expected-val expected))
         (let ((actual-val expr))
           (if (test-equal? expected-val actual-val)
               (begin
                 (test-increment-passed)
                 #t)
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
                 #f))))))))
