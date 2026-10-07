;; REPL-style: the same libraries imported again.
(import (scheme base) (scheme cxr) (scheme write) (scheme eval) (scheme repl) (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(steady-run (lambda (i) (eval '(import (scheme char) (scheme list)) env)))
