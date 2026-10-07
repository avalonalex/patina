;; Write a 1,000-line file with call-with-output-file, read it back line by
;; line with call-with-input-file; 500 times, every port closed.
(import (scheme base) (scheme file) (scheme write) (scheme process-context))
(define path (list-ref (command-line) 1))
(define (write-file n)
  (call-with-output-file path
    (lambda (p) (let loop ((i 0)) (when (< i n) (write i p) (newline p) (loop (+ i 1)))))))
(define (read-file)
  (call-with-input-file path
    (lambda (p) (let loop ((sum 0)) (let ((line (read-line p))) (if (eof-object? line) sum (loop (+ sum (string-length line)))))))))
(let loop ((k 0) (acc 0))
  (if (< k 500) (begin (write-file 1000) (loop (+ k 1) (+ acc (read-file))))
      (begin (display acc) (newline))))
