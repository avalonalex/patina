;; REPL-style: one quoted form, expanded by eval again and again (#612's
;; case among them). Argument 2 names the form.
(import (scheme base) (scheme cxr) (scheme write) (scheme eval) (scheme repl)
        (scheme lazy) (scheme case-lambda) (scheme process-context) (patina debug))
(include "driver.scm")
(define env (interaction-environment))
(define form
  (case (string->symbol (steady-arg 2))
    ((case) '(case 3 ((1 2) 'a) ((3) 'b) (else 'c)))
    ((guard) '(guard (e (#t 0)) (raise 'x)))
    ((let-values) '(let-values (((a b) (values 1 2))) (+ a b)))
    ((parameterize) '(let ((p (make-parameter 1))) (parameterize ((p 2)) (p))))
    ((case-lambda) '((case-lambda ((x) x) ((x y) (+ x y))) 1 2))
    ((delay) '(force (delay (+ 1 2))))
    ((do) '(do ((i 0 (+ i 1))) ((= i 3) i)))
    ((record) '(define-record-type point (make-point x y) point? (x point-x)))
    ((let) '(let loop ((i 0)) (if (< i 3) (loop (+ i 1)) i)))
    (else (error "unknown form" (steady-arg 2)))))
(steady-run (lambda (i) (eval form env)))
