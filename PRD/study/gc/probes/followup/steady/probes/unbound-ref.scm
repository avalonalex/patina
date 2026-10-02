(import (scheme base) (scheme write) (scheme eval) (scheme repl) (scheme process-context))
(define N (string->number (cadr (command-line))))
(define env (interaction-environment))
;; compile code that refers to a fresh unbound name, without running the reference
(let loop ((i 0))
  (when (< i N)
    (eval (list 'lambda '() (string->symbol (string-append "unbound-" (number->string i)))) env)
    (loop (+ i 1))))
(display 'done) (newline)
