(import (scheme base) (scheme write) (scheme process-context))
(define (count n) (if (= n 0) 0 (+ 1 (count (- n 1)))))
(define depth (string->number (cadr (command-line))))
(define r
  (call-with-current-continuation
    (lambda (k)
      (with-exception-handler
        (lambda (e) (k (list 'caught (if (error-object? e) (error-object-message e) e))))
        (lambda () (count depth))))))
(write r) (newline)
