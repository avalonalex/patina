(import (scheme base) (scheme write))
(define (churn n) (let loop ((i 0) (acc '())) (if (= i n) 0 (loop (+ i 1) (if (= (remainder i 1000) 0) '() (cons i acc))))))
(display (churn 20000000))
