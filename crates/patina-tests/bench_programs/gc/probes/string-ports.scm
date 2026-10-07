;; 1,000,000 output string ports, each written and read back.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(let loop ((i 0) (acc 0))
  (if (< i (arg 1 1000000))
      (let ((out (open-output-string)))
        (write i out)
        (loop (+ i 1) (+ acc (string-length (get-output-string out)))))
      (begin (display acc) (newline))))
