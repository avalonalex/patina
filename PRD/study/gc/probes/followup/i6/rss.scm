(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(define n (string->number (cadr (command-line))))
(length (make-list n 0))          ; the peak, dropped at once
(gc) (cons 1 2) (gc) (cons 1 2)
(display "idle") (newline) (flush-output-port)
(read-line)
