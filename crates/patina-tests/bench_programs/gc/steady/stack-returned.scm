;; Server-style: a deep recursion's stack, returned (PRD/GC_PRD.md §19's
;; row 4d and §17.4's limits row; #910). One non-tail recursion, argument 2
;; deep, under the default cap: 10 M frames on the VM, 1.36 GB of register
;; stack, and 1 M on the tree-walker, which keeps no such stack. It returns,
;; and after two (gc)s the row reads what the stack still holds and the cap
;; it ran under, which the runner holds to the row's bounds; then a loop
;; with little live data.
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(include "driver.scm")
(define (deep n) (if (= n 0) 0 (+ 1 (deep (- n 1)))))
(define depth (steady-number 2))
(unless (= (deep depth) depth) (error "the recursion answered wrong"))
(steady-bound-reading '(committed-stack-bytes stack-limit))
(define acc '())
(steady-run
 (lambda (i)
   (set! acc (if (= 0 (modulo i 1000)) '() (cons i acc)))))
