;; Churn: a continuation captured at a depth and dropped. Argument 2 is the
;; depth.
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(include "driver.scm")
(define depth (steady-number 2))
(define (deep n)
  (if (= n 0)
      (call-with-current-continuation (lambda (k) 1))
      (+ 1 (deep (- n 1)))))
(steady-run (lambda (i) (deep depth)))
