(import (scheme base) (scheme write))
(define (f p) (car p))
(import (rename (only (scheme base) cdr) (cdr car)))
(write (list (f '(1 2)) (car '(1 2)))) (newline)
