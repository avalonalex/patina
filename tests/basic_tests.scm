; Basic arithmetic tests
(+ 1 2 3)                    ; => 6
(- 10 3)                     ; => 7
(* 2 3 4)                    ; => 24
(/ 20 4)                     ; => 5

; List operations
(cons 1 2)                   ; => (1 . 2)
(cons 1 (cons 2 (cons 3 ()))) ; => (1 2 3)
(car (cons 1 2))             ; => 1
(cdr (cons 1 2))             ; => 2

; Predicates
(null? ())                   ; => #t
(null? 42)                   ; => #f
(pair? (cons 1 2))           ; => #t
(pair? 42)                   ; => #f

; Variables
(define x 42)
x                            ; => 42

; Conditionals
(if #t 1 2)                  ; => 1
(if #f 1 2)                  ; => 2
(if 0 1 2)                   ; => 1 (0 is truthy in Scheme)

; Begin
(begin
  (define y 10)
  (+ y 5))                   ; => 15
