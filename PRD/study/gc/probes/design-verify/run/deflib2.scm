(import (scheme base) (scheme write))
(define (define-library . args) (write (list 'called args)) (newline))
(define-library 1 2)
(write 'end) (newline)
