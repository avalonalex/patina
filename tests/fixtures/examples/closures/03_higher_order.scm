;; Higher-Order Functions with Closures
;; Demonstrates functions that take and return functions

;; Example 1: Function composition
(define compose
  (lambda (f g)
    (lambda (x) (f (g x)))))

(define add1 (lambda (x) (+ x 1)))
(define double (lambda (x) (* x 2)))
(define square (lambda (x) (* x x)))

(define add1-then-double (compose double add1))
(define double-then-square (compose square double))

(add1-then-double 5)   ; => 12  ((5 + 1) * 2)
(double-then-square 3) ; => 36  ((3 * 2)^2)

;; Example 2: Partial application (currying)
(define curry
  (lambda (f)
    (lambda (x)
      (lambda (y)
        (f x y)))))

(define add (lambda (x y) (+ x y)))
(define curried-add (curry add))
(define add7 ((curry add) 7))

(add7 3)   ; => 10
(add7 100) ; => 107

;; Example 3: Function that returns multiple closures
(define make-ops
  (lambda (n)
    (let ((value n))
      (lambda (op)
        (if (eq? op 'inc)
            (lambda (x) (+ x value))
            (if (eq? op 'dec)
                (lambda (x) (- x value))
                (lambda (x) (* x value))))))))

(define ops5 (make-ops 5))
(define inc5 (ops5 'inc))
(define dec5 (ops5 'dec))
(define mul5 (ops5 'mul))

(inc5 10) ; => 15
(dec5 10) ; => 5
(mul5 10) ; => 50

;; Expected output: 12, 36, 10, 107, 15, 5, 50
