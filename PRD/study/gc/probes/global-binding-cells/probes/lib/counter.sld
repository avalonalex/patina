(define-library (counter)
  (export count bump! get-count)
  (import (scheme base))
  (begin
    (define count 0)
    (define (bump!) (set! count (+ count 1)))
    (define (get-count) count)))
