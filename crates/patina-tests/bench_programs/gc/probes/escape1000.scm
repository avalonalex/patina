;; 20,000 escapes by call/cc from 10 frames down, under a 1,000-frame
;; non-tail stack that every capture holds (PRD/study/gc).
(import (scheme base) (scheme write))
(define (dive k d) (if (= d 0) (k 1) (+ 1 (dive k (- d 1)))))
(define (loop n acc) (if (= n 0) acc (loop (- n 1) (+ acc (call/cc (lambda (k) (dive k 10)))))))
(define (deep d) (if (= d 0) (loop 20000 0) (+ 0 (deep (- d 1)))))
(display (deep 1000))
