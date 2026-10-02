(import (scheme base) (scheme write) (patina debug))
(define (loop i acc) (if (= i 10000000) (car acc) (loop (+ i 1) (if (= 0 (remainder i 1000)) '() (cons i acc)))))
(write (loop 0 '())) (newline)
(write (assq 'collections (gc-stats))) (newline)
