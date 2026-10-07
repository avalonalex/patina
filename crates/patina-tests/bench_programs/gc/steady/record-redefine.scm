;; REPL-style: the same record type defined again, and used.
(import (scheme base) (scheme cxr) (scheme write) (scheme eval) (scheme repl) (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(steady-run
 (lambda (i)
   (eval '(define-record-type point (make-point x y) point? (x point-x) (y point-y)) env)
   (eval '(point-x (make-point 1 2)) env)))
