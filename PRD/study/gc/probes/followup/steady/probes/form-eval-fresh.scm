(import (scheme base) (scheme cxr) (scheme write) (scheme read) (scheme eval) (scheme repl) (scheme process-context))
;; same as form-eval, but the datum is re-read every iteration (a server loop
;; reading requests): no node persists across evals
(define N (string->number (cadr (command-line))))
(define text (caddr (command-line)))
(define env (interaction-environment))
(let loop ((i 0))
  (when (< i N)
    (eval (read (open-input-string text)) env)
    (loop (+ i 1))))
(display 'done) (newline)
