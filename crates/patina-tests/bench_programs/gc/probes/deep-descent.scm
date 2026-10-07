;; A recursion 1,000,000 frames deep that allocates nothing until the
;; bottom, where it builds garbage: a collection there scans the whole
;; descent.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define (churn n acc) (if (= n 0) (length acc) (churn (- n 1) (if (= 0 (modulo n 1000)) '() (cons n acc)))))
(define (descend d) (if (= d 0) (churn 2000000 '()) (+ 0 (descend (- d 1)))))
(let loop ((k 0) (sum 0))
  (if (< k 3)
      (loop (+ k 1) (+ sum (descend (arg 1 1000000))))
      (begin (display sum) (newline))))
