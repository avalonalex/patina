;; A non-tail map over 1,000,000 elements, five times: the stack grows to
;; a million frames and unwinds, with every frame live until it returns.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define (my-map f l) (if (null? l) '() (cons (f (car l)) (my-map f (cdr l)))))
(define big (let loop ((i 0) (acc '())) (if (= i (arg 1 1000000)) acc (loop (+ i 1) (cons i acc)))))
(let loop ((k 0) (sum 0))
  (if (< k 5)
      (loop (+ k 1) (+ sum (length (my-map (lambda (x) (+ x 1)) big))))
      (begin (display sum) (newline))))
