(import (scheme base) (scheme write) (scheme process-context))
;; Deep non-tail recursion: build and sum a 300k-element list recursively.
(define (build n) (if (= n 0) '() (cons n (build (- n 1)))))
(define (sum l) (if (null? l) 0 (+ (car l) (sum (cdr l)))))
(define (go k n acc) (if (= k 0) acc (go (- k 1) n (+ acc (sum (build n))))))
(define n (if (> (length (command-line)) 1) (string->number (cadr (command-line))) 1000000))
(display (go 10 n 0))
(newline)
