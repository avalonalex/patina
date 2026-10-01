(import (scheme base) (scheme write) (scheme eval) (scheme repl) (scheme process-context))
(define N (string->number (cadr (command-line))))
(define env (interaction-environment))
;; REPL-style: redefine a library and re-import it, again and again
(let loop ((i 0) (acc 0))
  (if (< i N)
      (begin
        (eval '(define-library (tmp steady) (export f g)
                 (import (scheme base))
                 (begin (define (f x) (+ x 1)) (define g (list 1 2 3))))
              env)
        (eval '(import (tmp steady)) env)
        (loop (+ i 1) (+ acc (eval '(f 1) env))))
      (begin (display acc) (newline))))
