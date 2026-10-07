;; REPL-style: the same file loaded again and again.
(import (scheme base) (scheme write) (scheme load) (scheme eval) (scheme repl)
        (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(steady-run (lambda (i) (load "loaded.scm" env)))
