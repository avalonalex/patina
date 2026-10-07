;; 20 MiB of garbage with almost nothing live: the floor of a program that
;; allocates a little.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(let loop ((i 0) (acc 0))
  (if (< i 600000)
      (loop (+ i 1) (+ acc (length (list i i))))
      (begin (display acc) (newline))))
