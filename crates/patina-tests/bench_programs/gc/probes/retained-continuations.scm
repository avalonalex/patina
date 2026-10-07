;; Continuations captured at depth 100 and kept: a ring of the last 1,000,
;; so most captures die young while the ring holds a fixed number live.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define ring (make-vector 1000 #f))
(define (capture i d)
  (if (= d 0)
      (call-with-current-continuation (lambda (k) (vector-set! ring (modulo i 1000) k) 0))
      (+ 0 (capture i (- d 1)))))
(let loop ((i 0) (n (arg 1 30000)))
  (if (< i n) (begin (capture i 100) (loop (+ i 1) n))))
(display (vector-length ring))
(newline)
