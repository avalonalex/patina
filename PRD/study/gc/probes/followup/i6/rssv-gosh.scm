(import (scheme base) (scheme write) (scheme process-context) (only (gauche base) gc))
(define n (string->number (cadr (command-line))))
(define (build n) (let loop ((i 0) (acc (quote ()))) (if (= i n) acc (loop (+ i 1) (cons (vector i i) acc))))) (length (build n))          ; the peak, dropped at once
(gc) (gc)
(display "idle") (newline) (flush-output-port)
(read-line)
