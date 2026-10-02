(library (counter2) (export get-count2) (import (chezscheme)) (define (get-count2) 'c2))
(define (later-import) (get-count2))
(import (counter2))
(write (guard (e (#t (list 'error (condition-message e)))) (later-import))) (newline)
