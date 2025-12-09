;; numbers.scm -- Numeric predicates for (scheme base)
;;
;; Numeric predicates (R7RS Section 6.2.6)

(define (zero? x)
  (= x 0))

(define (positive? x)
  (> x 0))

(define (negative? x)
  (< x 0))

(define (odd? x)
  (not (= (remainder x 2) 0)))

(define (even? x)
  (= (remainder x 2) 0))
