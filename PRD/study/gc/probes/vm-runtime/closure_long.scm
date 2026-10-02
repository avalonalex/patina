(import (scheme base) (scheme write))
(define (mk i) (lambda (x) (+ x i)))
(define (run n) (let loop ((i 0) (s 0)) (if (= i n) s (loop (+ i 1) ((mk i) s)))))
(display (run 60000000))
