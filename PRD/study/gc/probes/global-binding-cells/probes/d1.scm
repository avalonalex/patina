(import (scheme base) (scheme write) (scheme eval) (scheme repl))
(define (try thunk)
  (call-with-current-continuation
    (lambda (k) (with-exception-handler (lambda (x) (k 'caught)) thunk))))
(write (try (lambda () (eval '(begin (error "boom") (define list-copy 5)) (interaction-environment))))) (newline)
(write (try (lambda () (eval '(list-copy '(1 2)) (interaction-environment))))) (newline)
(write (try (lambda () (eval '(begin (error "boom") (define fresh-name 5)) (interaction-environment))))) (newline)
(write (try (lambda () (eval 'fresh-name (interaction-environment))))) (newline)
