(import (scheme base) (scheme write) (patina debug))
(define (loop i) (if (> i 0) (begin (make-string 100000 #\a) (loop (- i 1)))))
(loop 500)
(display (assq 'collections (gc-stats))) (display (assq 'strings (gc-stats))) (newline)
