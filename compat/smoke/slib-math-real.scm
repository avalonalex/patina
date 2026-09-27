(import (scheme base) (scheme complex) (prefix (slib math-real) r:)
        (patina compat smoke))
(define (near expected thunk)
  (guard (ex (else #f)) (< (abs (- expected (thunk))) 1e-12)))
(check-equal "real absolute value and its alias accept exact reals" '(7/3 7/3)
  (list (r:real-abs -7/3) (r:abs -7/3)))
(check-equal "exponential and logarithm base cases" #t
  (and (near 1 (lambda () (r:real-exp 0))) (near 0 (lambda () (r:real-ln 1)))
       (near 0 (lambda () (r:ln 1))) (near 3 (lambda () (r:real-log 2 8)))))
(check-equal "real trigonometric base cases" #t
  (and (near 0 (lambda () (r:real-sin 0))) (near 1 (lambda () (r:real-cos 0)))
       (near 0 (lambda () (r:real-tan 0))) (near 0 (lambda () (r:real-asin 0)))
       (near 0 (lambda () (r:real-acos 1)))))
(check-equal "one-argument real atan accepts a real operand" #t
  (near 0 (lambda () (r:real-atan 0))))
(check-equal "two-argument real atan retains quadrant information" #t
  (near 2.356194490192345 (lambda () (r:real-atan 1 -1))))
(check-equal "the exported atan remains available" #t
  (near 0 (lambda () (r:atan 0))))
(check-equal "real square roots include exact rationals and zero" #t
  (and (near 3/2 (lambda () (r:real-sqrt 9/4))) (near 0 (lambda () (r:real-sqrt 0)))))
(check-equal "real powers accept negative bases with integral exponents" '(-8 16 1/4)
  (list (r:real-expt -2 3) (r:real-expt -2 4) (r:real-expt 2 -2)))
(check-equal "real powers accept positive bases and fractional exponents" #t
  (near 2 (lambda () (r:real-expt 4 1/2))))
(check-equal "real division operations work on fractions" '(-2 -3/2 3/2)
  (list (r:quo -15/2 3) (r:rem -15/2 3) (r:mod -15/2 3)))
(check-equal "real modulo follows a negative divisor" -3/2 (r:mod 15/2 -3))
(check-error "real absolute value rejects non-real numbers" (r:real-abs (make-rectangular 1 2)))
(check-error "real sine rejects non-real numbers" (r:real-sin (make-rectangular 1 2)))
(check-error "real square roots reject negative reals" (r:real-sqrt -1))
(check-error "real logarithms reject negative arguments" (r:real-ln -1))
(check-error "based logarithms reject nonpositive bases" (r:real-log 0 2))
(check-error "inverse sine rejects an out-of-range real" (r:real-asin 2))
(check-error "inverse cosine rejects an out-of-range real" (r:real-acos -2))
(check-error "negative bases reject fractional real powers" (r:real-expt -2 1/2))
(check-error "real atan rejects extra coordinates" (r:real-atan 1 2 3))
(smoke-finish)
