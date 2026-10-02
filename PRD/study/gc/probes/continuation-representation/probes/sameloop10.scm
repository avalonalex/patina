(import (scheme base) (scheme write))
(define (loop n acc) (if (= n 0) acc (loop (- n 1) (+ acc ((lambda (k) 1) 0)))))
(define (deep d) (if (= d 0) (loop 20000 0) (+ 0 (deep (- d 1)))))
(display (deep 10))
