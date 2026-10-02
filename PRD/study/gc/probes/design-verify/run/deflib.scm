(import (scheme base) (scheme write))
(define (define-library . args) (list 'called args))
(write (define-library 1 2)) (newline)
