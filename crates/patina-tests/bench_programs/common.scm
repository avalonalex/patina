;;; Common definitions for R7RS benchmarks
;;; Simplified from ecraven/r7rs-benchmarks/src/common.scm

;; The hide function - prevents compiler optimizations by obscuring the return path
;; This is important for fair benchmarking across implementations
(define (hide r x)
  (call-with-values
   (lambda ()
     (values (vector values (lambda (x) x))
             (if (< r 100) 0 1)))
   (lambda (v i)
     ((vector-ref v i) x))))

;; Run a thunk n times, return the final result
(define (run-benchmark name count thunk ok?)
  (let loop ((i 0) (result #f))
    (if (< i count)
        (loop (+ i 1) (thunk))
        (if (ok? result)
            result
            (error "benchmark failed" name result)))))
