(import (scheme base) (scheme write) (patina debug))
(define (loop i x) (if (= i 10000000) x (loop (+ i 1) (+ x 3))))
(write (loop 0 0)) (newline)
(write (assq 'collections (gc-stats))) (newline)
