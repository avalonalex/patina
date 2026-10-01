(import (scheme base) (scheme write))
(define (library . args) (write (list 'called args)) (newline))
(library 1 2)
(write 'end) (newline)
