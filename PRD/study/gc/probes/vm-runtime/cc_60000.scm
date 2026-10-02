(import (scheme base) (scheme write))
(define (deep d) (if (= d 0) (call/cc (lambda (c) 0)) (+ 1 (deep (- d 1)))))
(define (run n acc) (if (= n 0) acc (run (- n 1) (+ acc (deep 100)))))
(display (run 60000 0))
