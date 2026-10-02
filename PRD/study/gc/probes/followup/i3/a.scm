(import (scheme base) (scheme eval) (scheme repl) (scheme process-context) (scheme write))
(define-syntax def-counter
  (syntax-rules ()
    ((_ name) (begin (define state 0)
                     (define (name) (set! state (+ state 1)) state)))))
(define n (string->number (cadr (command-line))))
(do ((i 0 (+ i 1))) ((= i n))
  (eval '(def-counter counter) (interaction-environment)))
(display (counter)) (newline)
