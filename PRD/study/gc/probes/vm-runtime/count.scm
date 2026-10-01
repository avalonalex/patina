(import (scheme base) (scheme write))
(define (count n) (let loop ((i 0) (s 0)) (if (= i n) s (loop (+ i 1) (+ s i)))))
(display (count 20000000))
