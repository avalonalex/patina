;; Churn: an environment specifier made, used once and dropped (#615).
(import (scheme base) (scheme write) (scheme eval) (scheme process-context) (patina debug))
(include "driver.scm")
(steady-run (lambda (i) (eval '(+ 1 2) (environment '(scheme base)))))
