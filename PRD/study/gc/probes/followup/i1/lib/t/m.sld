(define-library (t m)
  (export via-public public-helper via-private)
  (import (scheme base))
  (begin
    (define (public-helper x) x)
    (define (private-helper x) x)
    (define-syntax via-public (syntax-rules () ((_ x) (public-helper x))))
    (define-syntax via-private (syntax-rules () ((_ x) (private-helper x))))))
