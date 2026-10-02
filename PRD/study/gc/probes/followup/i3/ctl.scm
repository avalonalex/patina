(import (scheme base) (scheme eval) (scheme repl) (scheme process-context) (scheme write))
(define-syntax def-plain
  (syntax-rules ()
    ((_ name) (begin (define (name) 0)))))
(define n (string->number (cadr (command-line))))
(do ((i 0 (+ i 1))) ((= i n))
  (eval '(def-plain counter) (interaction-environment)))
(display (counter)) (newline)
