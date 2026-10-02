(define test-vec (make-vector 1000 42)) (define (sum-vec v n acc) (if (= n 0) acc (sum-vec v (- n 1) (+ acc (vector-ref v (- n 1)))))) (sum-vec test-vec 1000 0)
