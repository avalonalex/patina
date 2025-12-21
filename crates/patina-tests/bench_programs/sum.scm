;;; SUM -- Sums integers from 0 to n.
;;; From ecraven/r7rs-benchmarks, a KVW benchmark.
;;;
;;; Tests tail recursion and integer arithmetic.

(define (run n)
  (let loop ((i n) (sum 0))
    (if (< i 0)
        sum
        (loop (- i 1) (+ i sum)))))

;; Standard benchmark: (run 10000) for 200000 iterations
;; Smaller for quick testing: (run 10000) for 100 iterations
