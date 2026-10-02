(import (scheme base) (scheme write) (scheme eval) (scheme process-context))
(define N (string->number (cadr (command-line))))
;; a fresh environment per iteration, and a procedure compiled in it
(let loop ((i 0) (acc 0))
  (if (< i N)
      (let* ((e (environment '(scheme base)))
             (g (eval '(lambda (x) (* x 2)) e)))
        (loop (+ i 1) (+ acc (g 21))))
      (begin (display acc) (newline))))
