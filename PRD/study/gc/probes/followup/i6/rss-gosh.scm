(import (scheme base) (scheme write) (scheme process-context) (only (gauche base) gc))
(define n (string->number (cadr (command-line))))
(length (make-list n 0))          ; the peak, dropped at once
(gc) (gc)
(display "idle") (newline) (flush-output-port)
(read-line)
