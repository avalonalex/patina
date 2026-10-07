;; A call/cc re-entry loop: the same continuation re-entered until a
;; counter runs out, allocating as it goes.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define (run n)
  (let ((k #f) (count 0) (acc '()))
    (call-with-current-continuation (lambda (c) (set! k c)))
    (set! count (+ count 1))
    (set! acc (cons count (if (pair? acc) (cdr acc) '())))
    (if (< count n) (k #f) count)))
(display (run (arg 1 2000000)))
(newline)
