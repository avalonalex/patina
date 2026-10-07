;; A non-tail recursion as deep as argument 1 (default 200,000), for the
;; tree-walker subset and the 10 M-deep row of #647.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define (deep d) (if (= d 0) 0 (+ 1 (deep (- d 1)))))
(display (deep (arg 1 200000)))
(newline)
