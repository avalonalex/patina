(import (chezscheme))
(define (nest n) (if (= n 0) 'a (list 'let '((a 1)) (nest (- n 1)))))
(write (guard (e (#t (list 'refused (condition-message e)))) (eval (nest 100000))))
(newline)
