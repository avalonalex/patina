;;; Complex Polynomial Evaluation
;;; Test complex numbers with polynomial arithmetic

;; Evaluate polynomial at a complex point
;; p(z) = z³ - 2z² + z - 1
(define (poly z)
  (+ (* z (* z z))
     (* -2 (* z z))
     z
     -1))

;; Test with various complex inputs
(poly 1+0i)     ; Real input: 1 - 2 + 1 - 1 = -1
(poly 0+1i)     ; Pure imaginary: -i - 2(-1) + i - 1 = 1
(poly 1+1i)     ; Complex input

;; Verify polynomial identity: (z - w)(z + w) = z² - w²
(define z 3+4i)
(define w 1+2i)
(define lhs (* (- z w) (+ z w)))
(define rhs (- (* z z) (* w w)))
;; Check: lhs and rhs should be equal (or very close)
lhs
rhs
(- lhs rhs)  ; Should be 0 or very small

;; Complex conjugate property: (a + bi)(a - bi) = a² + b²
(define a+bi 3+4i)
(define a-bi 3-4i)
(* a+bi a-bi)  ; Should be 9 + 16 = 25
