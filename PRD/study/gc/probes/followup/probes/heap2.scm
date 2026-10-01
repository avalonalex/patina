(import (scheme base) (scheme write))
(define (fill acc i) (if (= i 0) acc (fill (cons (make-vector 64 i) acc) (- i 1))))
(define r
  (call-with-current-continuation
    (lambda (k)
      (with-exception-handler
        (lambda (e) (k (list 'caught (if (error-object? e) (error-object-message e) e))))
        (lambda () (length (fill '() 20000000)))))))
(write r) (newline)
