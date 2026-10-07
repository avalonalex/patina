;; Churn: an environment specifier made, a procedure compiled in it and
;; called, and both dropped (#615).
(import (scheme base) (scheme write) (scheme eval) (scheme process-context) (patina debug))
(include "driver.scm")
(steady-run
 (lambda (i)
   ((eval '(lambda (x) (* x 2)) (environment '(scheme base))) 21)))
