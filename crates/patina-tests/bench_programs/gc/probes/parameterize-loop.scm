;; parameterize in a loop, read inside.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define p (make-parameter 0))
(let loop ((i 0) (acc 0))
  (if (< i (arg 1 300000))
      (loop (+ i 1) (+ acc (parameterize ((p i)) (p))))
      (begin (display acc) (newline))))
