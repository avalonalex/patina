(import (scheme base) (scheme write))
;; internal defines captured by closures => boxed MutableCells written once
(define (mk n)
  (define (even? k) (if (= k 0) #t (odd? (- k 1))))
  (define (odd? k) (if (= k 0) #f (even? (- k 1))))
  (define acc '())
  (let loop ((i 0)) (when (< i n) (set! acc (cons (even? i) acc)) (loop (+ i 1))))
  (length acc))
(let loop ((i 0) (s 0)) (if (< i 3000) (loop (+ i 1) (+ s (mk 30))) (begin (display s) (newline))))
