(import (scheme base) (scheme write) (patina debug))
(define (loop i) (if (> i 0) (begin (make-vector 100000 i) (loop (- i 1)))))
(loop 500)
(display (assq 'collections (gc-stats))) (display (assq 'vectors (gc-stats))) (newline)
