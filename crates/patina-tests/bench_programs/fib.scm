;;; FIB -- A classic benchmark, computes fib(n) inefficiently.
;;; From ecraven/r7rs-benchmarks, a Gabriel benchmark.
;;;
;;; Tests non-tail recursion and arithmetic.

(define (fib n)
  (if (< n 2)
      n
      (+ (fib (- n 1))
         (fib (- n 2)))))

;; Standard benchmark: (fib 40) for 5 iterations
;; Smaller for quick testing: (fib 30) or (fib 25)
