(import (scheme base) (scheme write) (srfi 69))
;; eq?-keyed SRFI 69 table (the R6RS make-eq-hashtable path): keys are fresh
;; pairs, so every insert and lookup goes through identity-hash.
(define (round n)
  (let ((t (make-hash-table eq?))
        (keys (let loop ((i 0) (acc '())) (if (= i n) acc (loop (+ i 1) (cons (list i) acc))))))
    (for-each (lambda (k) (hash-table-set! t k (car k))) keys)
    (let loop ((ks keys) (sum 0))
      (if (null? ks) sum (loop (cdr ks) (+ sum (hash-table-ref/default t (car ks) 0)))))))
(let loop ((r 0) (acc 0))
  (if (< r 5) (loop (+ r 1) (+ acc (round 100000)))
      (begin (display acc) (newline))))
