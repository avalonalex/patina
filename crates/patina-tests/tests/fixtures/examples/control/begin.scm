;; Begin tests - R7RS compliant
(import (scheme base)
        (scheme write))

;; Basic begin - returns last expression
(write (begin 1 2 3)) (newline)            ; 3

;; Begin with side effects
(begin
  (define x 10)
  (define y 20)
  (write (+ x y)) (newline))               ; 30

;; Nested begin
(write (begin
         (begin
           (define a 5)
           a)
         (+ a 10))) (newline)              ; 15
