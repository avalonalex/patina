;; Patina Demo - R7RS Compliant Version
;; This file demonstrates the features of Patina and can be run
;; with both Patina and chibi-scheme for comparison

(import (scheme base)
        (scheme write))

(display "=== Patina Scheme Interpreter Demo ===") (newline) (newline)

;; Basic arithmetic
(display "Basic arithmetic:") (newline)
(display "(+ 1 2 3) => ") (write (+ 1 2 3)) (newline)
(display "(- 10 3) => ") (write (- 10 3)) (newline)
(display "(* 2 3 4) => ") (write (* 2 3 4)) (newline)
(display "(/ 20 5) => ") (write (/ 20 5)) (newline)
(newline)

;; Define variables
(display "Variables:") (newline)
(define x 42)
(define y 58)
(display "x = ") (write x) (newline)
(display "y = ") (write y) (newline)
(display "(+ x y) => ") (write (+ x y)) (newline)
(newline)

;; Conditionals
(display "Conditionals:") (newline)
(display "(if (< x 50) \"less\" \"greater\") => ")
(write (if (< x 50)
    "less than 50"
    "greater than or equal to 50"))
(newline)
(newline)

;; Multi-line expressions with begin
(display "Multi-line with begin:") (newline)
(define z 100)
(display "(begin (define z 100) (+ z 50)) => ")
(write (+ z 50))
(newline)
(newline)

;; Lists
(display "Lists:") (newline)
(display "(cons 1 (cons 2 (cons 3 '()))) => ")
(write (cons 1 (cons 2 (cons 3 '())))) (newline)

(display "(list 1 2 3 4 5) => ")
(write (list 1 2 3 4 5)) (newline)

(display "(car (list 10 20 30)) => ")
(write (car (list 10 20 30))) (newline)

(display "(cdr (list 10 20 30)) => ")
(write (cdr (list 10 20 30))) (newline)
(newline)

;; More complex list operations
(display "List predicates:") (newline)
(define my-list (list 1 2 3 4 5))
(display "(null? my-list) => ")
(write (null? my-list)) (newline)

(display "(null? '()) => ")
(write (null? '())) (newline)

(display "(pair? my-list) => ")
(write (pair? my-list)) (newline)
(newline)

;; Nested structures
(display "Nested structures:") (newline)
(define nested '((1 2) (3 4) (5 6)))
(display "(car (car nested)) => ")
(write (car (car nested))) (newline)
(newline)

;; String literals
(display "Strings:") (newline)
(define greeting "Hello, Patina!")
(display "greeting => ")
(write greeting) (newline)
(newline)

;; Boolean values
(display "Booleans:") (newline)
(display "#t => ") (write #t) (newline)
(display "#f => ") (write #f) (newline)
(display "(if #t \"yes\" \"no\") => ")
(write (if #t "yes" "no")) (newline)
(newline)

(display "=== Demo Complete ===") (newline)
