;;; ACK -- Ackermann's function.
;;; From ecraven/r7rs-benchmarks.
;;;
;;; Tests deep recursion and stack management.

(define (ack m n)
  (cond ((= m 0) (+ n 1))
        ((= n 0) (ack (- m 1) 1))
        (else (ack (- m 1) (ack m (- n 1))))))

;; Standard benchmark: (ack 3 12) for 2 iterations
;; Note: ack(4, n) grows extremely fast, avoid n > 1
;; Smaller for quick testing: (ack 3 8)
