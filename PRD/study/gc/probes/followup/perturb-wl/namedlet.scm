(import (scheme base) (scheme write))
(define (sum n) (let loop ((i 0) (s 0)) (if (= i n) s (loop (+ i 1) (+ s i)))))
(display (sum 20000000))
