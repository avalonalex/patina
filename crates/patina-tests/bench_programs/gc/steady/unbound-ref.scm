;; REPL-style: a procedure naming a fresh unbound variable, compiled and
;; never called.
(import (scheme base) (scheme cxr) (scheme write) (scheme eval) (scheme repl) (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(steady-run
 (lambda (i)
   (eval (list 'lambda '() (string->symbol (string-append "unbound-" (number->string i))))
         env)))
