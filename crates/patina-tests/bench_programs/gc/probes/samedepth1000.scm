;; 20,000 call/cc captures at depth 1,000, each dropped at once (#606):
;; what a capture costs when it copies the whole stack.
(import (scheme base) (scheme write))
(define (loop n acc) (if (= n 0) acc (loop (- n 1) (+ acc (call/cc (lambda (k) 1))))))
(define (deep d) (if (= d 0) (loop 20000 0) (+ 0 (deep (- d 1)))))
(display (deep 1000))
