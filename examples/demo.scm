; Patina REPL Demo
; This file demonstrates the features of the Patina Scheme interpreter
;
; NOTE: This file is designed for the Patina REPL (interactive mode).
; For a version that runs as a complete R7RS script with chibi-scheme,
; see demo_r7rs.scm

; Basic arithmetic
(+ 1 2 3)                    ; => 6
(- 10 3)                     ; => 7
(* 2 3 4)                    ; => 24
(/ 20 5)                     ; => 4

; Define variables
(define x 42)
(define y 58)
(+ x y)                      ; => 100

; Conditionals
(if (< x 50)
    "less than 50"
    "greater than or equal to 50")

; Multi-line expressions (works great with syntax highlighting!)
(if (> x 0)
    (begin
      (define result "positive")
      result)
    "negative or zero")

; Lists
(cons 1 (cons 2 (cons 3 '())))
(list 1 2 3 4 5)
(car (list 10 20 30))        ; => 10
(cdr (list 10 20 30))        ; => (20 30)

; More complex list operations
(define my-list (list 1 2 3 4 5))
(null? my-list)              ; => #f
(null? '())                  ; => #t
(pair? my-list)              ; => #t

; Nested structures
(define nested '((1 2) (3 4) (5 6)))
(car (car nested))           ; => 1

; String literals (with syntax highlighting)
(define greeting "Hello, Patina!")
greeting

; Boolean values
#t                           ; => #t
#f                           ; => #f
(if #t "yes" "no")          ; => "yes"

; Note: Lambda, let, cond, and other forms are not yet implemented
; See NEXT_STEPS.md for the roadmap!
