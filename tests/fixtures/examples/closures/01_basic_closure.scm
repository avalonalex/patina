;; Basic Closure Example
;; Demonstrates lexical scoping and variable capture

;; Example 1: Simple closure with let
(define add4
  (let ((x 4))
    (lambda (y) (+ x y))))

(add4 6)    ; => 10
(add4 100)  ; => 104

;; Example 2: Closure capturing function parameter
(define make-adder
  (lambda (n)
    (lambda (x) (+ x n))))

(define add5 (make-adder 5))
(define add10 (make-adder 10))

(add5 3)   ; => 8
(add10 3)  ; => 13

;; Example 3: Each closure has independent environment
(define counter1 (make-adder 100))
(define counter2 (make-adder 200))

(counter1 1)  ; => 101
(counter2 1)  ; => 201

;; Expected output: 10, 104, 8, 13, 101, 201
