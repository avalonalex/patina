;;; TAK -- A triply recursive integer function related to the Takeuchi function.
;;; From ecraven/r7rs-benchmarks, a Gabriel benchmark.
;;;
;;; A test of non-tail calls and arithmetic.

(define (tak x y z)
  (if (not (< y x))
      z
      (tak (tak (- x 1) y z)
           (tak (- y 1) z x)
           (tak (- z 1) x y))))

;; Standard benchmark parameters: (tak 40 20 11) for 1 iteration
;; Smaller for quick testing: (tak 18 12 6)
