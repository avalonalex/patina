;; REPL-style: a macro whose expansion introduces a hidden top-level
;; definition, expanded again under the same visible name (#613).
(import (scheme base) (scheme cxr) (scheme write) (scheme eval) (scheme repl) (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(eval '(define-syntax def-counter
         (syntax-rules ()
           ((_ name) (begin (define state 0)
                            (define (name) (set! state (+ state 1)) state)))))
      env)
(steady-run
 (lambda (i)
   (eval '(def-counter counter) env)
   (eval '(counter) env)))
