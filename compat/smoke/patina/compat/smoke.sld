;; Test-only assertion protocol; supplied by patina-compat, never bundled.
(define-library (patina compat smoke)
  (export check-equal check-error smoke-finish)
  (import (scheme base) (scheme write))
  (begin
    (define smoke-passed 0)
    (define smoke-failed 0)
    (define (check-equal label expected actual)
      (if (equal? expected actual)
          (set! smoke-passed (+ smoke-passed 1))
          (begin
            (set! smoke-failed (+ smoke-failed 1))
            (display "FAIL: ") (display label)
            (display " expected ") (write expected)
            (display " got ") (write actual) (newline))))
    (define-syntax check-error
      (syntax-rules ()
        ((_ label expression)
         (check-equal label #t
           (guard (ex (else #t)) expression #f)))))
    (define (smoke-finish)
      (write (list 'patina-compat-smoke smoke-passed smoke-failed))
      (newline))
  ))
