;; An allocating mutual tail recursion with constant windows: under
;; PATINA_GC_STRESS it must reach a safe point every iteration.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define (my-even? n acc) (if (= n 0) (length acc) (my-odd? (- n 1) (cons n (if (pair? acc) (cdr acc) '())))))
(define (my-odd? n acc) (if (= n 0) (length acc) (my-even? (- n 1) (cons n acc))))
(display (my-even? (arg 1 10000000) '()))
(newline)
