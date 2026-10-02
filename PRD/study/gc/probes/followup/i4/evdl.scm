(import (scheme base) (scheme write) (scheme eval) (scheme repl))
(eval '(define-library (tmp ev) (export f) (import (scheme base)) (begin (define (f x) (+ x 1)))) (interaction-environment))
(eval '(import (tmp ev)) (interaction-environment))
(display (eval '(f 1) (interaction-environment))) (newline)
