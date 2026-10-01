(import (scheme base) (scheme write) (scheme process-context) (srfi 18))
(define N (string->number (cadr (command-line))))
;; start, join and drop N threads: churn with bounded live threads
(let loop ((i 0) (acc 0))
  (if (< i N)
      (let ((t (make-thread (lambda () (* i 2)))))
        (thread-start! t)
        (loop (+ i 1) (+ acc (thread-join! t))))
      (begin (display acc) (newline))))
