;; Churn, class D: a fresh top-level name per cycle. Outside the contract,
;; since the program keeps every name; the lane holds its cost per name.
(import (scheme base) (scheme cxr) (scheme write) (scheme eval) (scheme repl) (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(steady-run
 (lambda (i)
   (eval (list 'define (string->symbol (string-append "fresh-var-" (number->string i))) i)
         env)))
