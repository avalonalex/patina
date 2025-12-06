;; (chibi test) - Test framework library
;;
;; Provides testing functionality compatible with chibi-scheme's test suite.
;; The primitive procedures are in (chibi test primitives), this file adds
;; the test macro and helper functions.

(define-library (chibi test)
  (import (scheme base)
          (scheme complex)
          (scheme write)
          (chibi test primitives))
  (export test test-begin test-end test-values
          test-increment-passed test-increment-failed
          test-equal?)
  (begin
    ;; test-equal? - Helper for approximate equality
    ;;
    ;; Supports:
    ;; - Approximate comparison for inexact real numbers (using epsilon = 1e-6)
    ;; - Approximate comparison for inexact complex numbers (compares real and imaginary parts)
    ;; - Recursive comparison for pairs and vectors
    ;; - Exact comparison for everything else
    (define (test-equal? expected actual)
      (define epsilon 1e-6)

      ;; Helper predicates
      (define (is-nan? x) (and (real? x) (inexact? x) (not (= x x))))
      (define (is-infinite? x) (and (real? x) (inexact? x) (not (is-nan? x)) (= x (* 2 x))))

      (define (approx-equal? x y)
        (cond
          ;; For inexact complex numbers (non-real), compare real and imaginary parts
          ((and (complex? x) (complex? y)
                (not (real? x)) (not (real? y))
                (or (inexact? x) (inexact? y)))
           (and (approx-equal? (real-part x) (real-part y))
                (approx-equal? (imag-part x) (imag-part y))))
          ;; For real inexact numbers, use epsilon-based comparison
          ((and (real? x) (real? y) (inexact? x) (inexact? y))
           (cond
             ((and (is-nan? x) (is-nan? y)) #t)
             ((or (is-nan? x) (is-nan? y)) #f)
             ((and (is-infinite? x) (is-infinite? y)) (eqv? x y))
             ((or (is-infinite? x) (is-infinite? y)) #f)
             (else (< (abs (- x y)) epsilon))))
          ;; For pairs, recurse
          ((and (pair? x) (pair? y))
           (and (approx-equal? (car x) (car y))
                (approx-equal? (cdr x) (cdr y))))
          ;; For vectors, recurse
          ((and (vector? x) (vector? y) (= (vector-length x) (vector-length y)))
           (let loop ((i 0))
             (or (>= i (vector-length x))
                 (and (approx-equal? (vector-ref x i) (vector-ref y i))
                      (loop (+ i 1))))))
          ;; Everything else: exact equal?
          (else (equal? x y))))

      (approx-equal? expected actual))

    ;; test-values - Test for multiple values
    (define-syntax test-values
      (syntax-rules ()
        ((test-values expect expr)
         (test-values #f expect expr))
        ((test-values name expect expr)
         (test (call-with-values (lambda () expect) (lambda results results))
               (call-with-values (lambda () expr) (lambda results results))))))

    ;; test - Main test macro
    (define-syntax test
      (syntax-rules ()
        ((test expected expr)
         (begin
           (display "Testing: ")
           (newline)
           (write 'expr)
           (newline)
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
                     #f))))))))))
