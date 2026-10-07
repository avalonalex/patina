;; Server-style: one deep non-tail recursion, which returns, then a loop
;; with little live data. Argument 2 is the depth.
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(include "driver.scm")
(define (deep n) (if (= n 0) 0 (+ 1 (deep (- n 1)))))
(deep (steady-number 2))
(define acc '())
(steady-run
 (lambda (i)
   (set! acc (if (= 0 (modulo i 1000)) '() (cons i acc)))))
