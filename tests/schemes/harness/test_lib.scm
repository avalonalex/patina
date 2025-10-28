;; Simple test harness for Scheme tests
;; This library provides utilities for writing tests that work with both
;; Patina and chibi-scheme

(import (scheme base)
        (scheme write))

;; Test counter
(define test-count 0)
(define pass-count 0)
(define fail-count 0)

;; Test a single expression
(define (test-equal name expected actual)
  (set! test-count (+ test-count 1))
  (display name)
  (display ": ")
  (if (equal? expected actual)
      (begin
        (set! pass-count (+ pass-count 1))
        (display "PASS")
        (newline))
      (begin
        (set! fail-count (+ fail-count 1))
        (display "FAIL - expected ")
        (write expected)
        (display " but got ")
        (write actual)
        (newline))))

;; Print test summary
(define (test-summary)
  (newline)
  (display "Tests: ")
  (display test-count)
  (display ", Passed: ")
  (display pass-count)
  (display ", Failed: ")
  (display fail-count)
  (newline))
