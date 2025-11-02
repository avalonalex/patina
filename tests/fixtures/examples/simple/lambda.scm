;; Lambda and closure tests
;; Test basic lambda functionality

;; Basic lambda - identity function
((lambda (x) x) 42)

;; Lambda with arithmetic
((lambda (x y) (+ x y)) 3 4)

;; Lambda with multiple body expressions
((lambda (x) (+ x 1) (* x 2)) 5)

;; Closure - lambda captures environment
(define make-adder
  (lambda (n)
    (lambda (x) (+ x n))))

(define add5 (make-adder 5))
(add5 10)

;; Nested lambdas
(((lambda (x) (lambda (y) (+ x y))) 3) 4)

;; Lambda with no parameters
((lambda () 42))

;; Lambda with multiple parameters
((lambda (a b c) (+ a (* b c))) 1 2 3)

;; Higher-order function - function returns function
(define twice
  (lambda (f)
    (lambda (x) (f (f x)))))

(define add1 (lambda (x) (+ x 1)))
((twice add1) 10)

;; Closure captures multiple variables
(define make-counter
  (lambda (start step)
    (lambda (n)
      (+ start (* step n)))))

(define counter (make-counter 100 10))
(counter 5)

;; Lambda as argument
(define apply-twice
  (lambda (f x)
    (f (f x))))

(apply-twice (lambda (n) (* n 2)) 3)

;; Variadic lambda - all args
((lambda args args) 1 2 3)

;; Variadic lambda with fixed params
((lambda (x . rest) rest) 1 2 3 4)
((lambda (x . rest) x) 1 2 3)

;; Zero-argument variadic
((lambda args args))

;; Complex closure - lexical scoping
(define x 10)
(define f (lambda (y) (+ x y)))
(define x 20)
(f 5)
