;; A library file cut short inside its define-library form.
(define-library (test truncated)
  (import (scheme base))
  (export half)
  (begin
    (define (half x) (/ x 2
