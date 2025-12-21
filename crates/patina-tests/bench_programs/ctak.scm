;;; CTAK -- A version of the TAK benchmark that uses continuations.
;;; From ecraven/r7rs-benchmarks, a Gabriel benchmark.
;;;
;;; Tests call/cc performance and continuation overhead.

(define (ctak x y z)
  (call-with-current-continuation
   (lambda (k)
     (ctak-aux k x y z))))

(define (ctak-aux k x y z)
  (if (not (< y x))
      (k z)
      (call-with-current-continuation
       (lambda (k)
         (ctak-aux
          k
          (call-with-current-continuation
           (lambda (k)
             (ctak-aux k (- x 1) y z)))
          (call-with-current-continuation
           (lambda (k)
             (ctak-aux k (- y 1) z x)))
          (call-with-current-continuation
           (lambda (k)
             (ctak-aux k (- z 1) x y))))))))

;; Standard benchmark: (ctak 18 12 6) for 1 iteration
;; This is much slower than tak due to continuation overhead
