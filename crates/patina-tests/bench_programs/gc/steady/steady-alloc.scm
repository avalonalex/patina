;; Server-style: bounded live data, a ring of 20,000 slots, each a 10-element
;; list, a 16-character string and a 4-element vector; one slot replaced per
;; cycle.
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(include "driver.scm")
(define K 20000)
(define ring (make-vector K #f))
(define (make-item i)
  (list (let build ((n 10) (acc '())) (if (= n 0) acc (build (- n 1) (cons (+ i n) acc))))
        (make-string 16 #\a)
        (vector i (* i 2) (* i 3) (inexact i))))
(steady-run (lambda (i) (vector-set! ring (modulo i K) (make-item i))))
