;; Closures Capturing Multiple Variables
;; Demonstrates capturing multiple variables from the environment

;; Example 1: Point object with getters
(define make-point
  (lambda (x y)
    (lambda (msg)
      (if (eq? msg 'x)
          x
          (if (eq? msg 'y)
              y
              (if (eq? msg 'sum)
                  (+ x y)
                  'unknown))))))

(define p1 (make-point 3 4))
(define p2 (make-point 10 20))

(p1 'x)   ; => 3
(p1 'y)   ; => 4
(p1 'sum) ; => 7
(p2 'x)   ; => 10
(p2 'sum) ; => 30

;; Example 2: Rectangle calculator
(define make-rectangle
  (lambda (width height)
    (lambda (operation)
      (if (eq? operation 'area)
          (* width height)
          (if (eq? operation 'perimeter)
              (* 2 (+ width height))
              (if (eq? operation 'diagonal)
                  ;; Using Pythagorean theorem (simplified as sum for now)
                  (+ width height)
                  'unknown))))))

(define rect (make-rectangle 5 3))
(rect 'area)      ; => 15
(rect 'perimeter) ; => 16

;; Example 3: Nested scope - capturing from multiple levels
(define make-calculator
  (lambda (x)
    (lambda (y)
      (lambda (z)
        (lambda (op)
          (if (eq? op 'sum)
              (+ x (+ y z))
              (if (eq? op 'product)
                  (* x (* y z))
                  'unknown)))))))

(define calc (((make-calculator 2) 3) 4))
(calc 'sum)     ; => 9  (2 + 3 + 4)
(calc 'product) ; => 24 (2 * 3 * 4)

;; Expected output: 3, 4, 7, 10, 30, 15, 16, 9, 24
