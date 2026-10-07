;; Churn: a form read afresh from text, with no source location, and
;; evaluated.
(import (scheme base) (scheme write) (scheme read) (scheme eval) (scheme repl)
        (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(steady-run
 (lambda (i)
   (eval (read (open-input-string "(let loop ((i 0)) (if (< i 3) (loop (+ i 1)) i))")) env)))
