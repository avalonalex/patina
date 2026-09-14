;; A complete define-library form, followed by a datum the file cuts short.
(define-library (test trailing-garbage)
  (import (scheme base))
  (export half)
  (begin
    (define (half x) (/ x 2))))
(define (orphan x)
