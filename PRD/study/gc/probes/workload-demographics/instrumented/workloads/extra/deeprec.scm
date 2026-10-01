(import (scheme base) (scheme write))
;; Deep non-tail recursion: build and sum a 300k-element list recursively.
(define (build n) (if (= n 0) '() (cons n (build (- n 1)))))
(define (sum l) (if (null? l) 0 (+ (car l) (sum (cdr l)))))
(define (go k n acc) (if (= k 0) acc (go (- k 1) n (+ acc (sum (build n))))))
(display (go 10 1000000 0))
(newline)
