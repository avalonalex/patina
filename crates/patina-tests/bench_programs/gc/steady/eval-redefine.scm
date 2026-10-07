;; REPL-style: the same definition evaluated again and again, and called.
(import (scheme base) (scheme write) (scheme eval) (scheme repl)
        (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(steady-run
 (lambda (i)
   (eval '(define (f x) (let ((y (* x 2))) (if (> y 10) (- y 1) (+ y 1)))) env)
   (eval '(f 3) env)))
