(import (scheme base) (scheme write))
(define (grow acc n) (if (= n 0) acc (grow (cons (make-vector 16 n) acc) (- n 1))))
(define r
  (call-with-current-continuation
    (lambda (k)
      (with-exception-handler
        (lambda (e) (k (list 'caught (if (error-object? e) (error-object-message e) e))))
        (lambda () (let loop ((acc '()) (i 0)) (if (= i 1000) (length acc) (loop (grow acc 100000) (+ i 1)))))))))
(write r) (newline)
