;; REPL-style: a procedure compiled by eval, called once, and dropped.
(import (scheme base) (scheme cxr) (scheme write) (scheme eval) (scheme repl) (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(steady-run
 (lambda (i)
   ((eval '(lambda (x) (let ((y (* x 2))) (if (> y 10) (- y 1) (+ y 1)))) env) i)))
