;; Churn: a procedure `eval` compiles from a datum that quotes a fresh
;; 1,000-element vector, called and dropped. The vector is a constant of
;; the procedure's code. On the VM a loaded unit's constants are roots
;; until the unit is released, after the collection that frees the
;; procedure, so that collection marks the vector, counts it in L and
;; frees it at the next: 8 KB of a cycle's 10, and the threshold,
;; max(8 MiB, 2L), follows it (docs/GC_DESIGN.md §6.2; #866).
(import (scheme base) (scheme write) (scheme eval) (scheme process-context) (patina debug))
(include "driver.scm")
(define env (environment '(scheme base)))
(steady-run
 (lambda (i)
   ((eval (list 'lambda '() (list 'vector-length (list 'quote (make-vector 1000 0))))
          env))))
