(import (scheme base) (scheme write))
(define (f) (later 1))
(define (later x) (list 'later x))
(write (f)) (newline)
