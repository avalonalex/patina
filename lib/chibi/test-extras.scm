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

  ;; Helper predicates that don't require (scheme inexact)
  ;; TODO: When the library system supports it, import (scheme inexact) and use
  ;; nan? and infinite? directly instead of these workarounds.
  ;; NaN is the only value not equal to itself
  (define (is-nan? x) (and (real? x) (inexact? x) (not (= x x))))
  ;; Infinity is larger than any finite number
  (define (is-infinite? x) (and (real? x) (inexact? x) (not (is-nan? x)) (= x (* 2 x))))

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
      ;; Handle special cases: NaN, infinity
      ((and (real? x) (real? y) (inexact? x) (inexact? y))
       (cond
         ;; Both NaN: NaN = NaN for testing purposes
         ((and (is-nan? x) (is-nan? y)) #t)
         ;; One NaN: not equal
         ((or (is-nan? x) (is-nan? y)) #f)
         ;; Both infinite with same sign
         ((and (is-infinite? x) (is-infinite? y))
          (eqv? x y))
         ;; One infinite: not equal
         ((or (is-infinite? x) (is-infinite? y)) #f)
         ;; Normal case: epsilon comparison
         (else (< (abs (- x y)) epsilon))))
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
;; Test macro for multiple values
;;
;; (test-values expect expr) - Tests that expr produces the same multiple values as expect
;; Both expect and expr are evaluated with call-with-values and their results compared.
(define-syntax test-values
  (syntax-rules ()
    ((test-values expect expr)
     (test-values #f expect expr))
    ((test-values name expect expr)
     (test (call-with-values (lambda () expect) (lambda results results))
           (call-with-values (lambda () expr) (lambda results results))))))

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
