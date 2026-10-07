;; Server-style: a peak of 2,000,000 two-element vectors, of which every
;; 4,096th is kept, then a loop with little live data (#616). Argument 2 is
;; the peak's length.
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(include "driver.scm")
(define (build n) (let loop ((i 0) (acc '())) (if (= i n) acc (loop (+ i 1) (cons (vector i i) acc)))))
(define kept
  (let loop ((l (build (steady-number 2))) (i 0) (acc '()))
    (cond ((null? l) acc)
          ((= 0 (modulo i 4096)) (loop (cdr l) (+ i 1) (cons (car l) acc)))
          (else (loop (cdr l) (+ i 1) acc)))))
(define acc '())
(steady-run
 (lambda (i)
   (set! acc (if (= 0 (modulo i 1000)) '() (cons i acc)))))
