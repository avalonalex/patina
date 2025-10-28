;; Basic list operation tests - R7RS compliant
(import (scheme base)
        (scheme write))

;; Test cons
(write (cons 1 2)) (newline)               ; (1 . 2)
(write (cons 1 '())) (newline)             ; (1)
(write (cons 1 (cons 2 (cons 3 '())))) (newline)  ; (1 2 3)

;; Test car and cdr
(write (car (cons 1 2))) (newline)         ; 1
(write (cdr (cons 1 2))) (newline)         ; 2
(write (car '(1 2 3))) (newline)           ; 1
(write (cdr '(1 2 3))) (newline)           ; (2 3)

;; Test list
(write (list)) (newline)                   ; ()
(write (list 1)) (newline)                 ; (1)
(write (list 1 2 3 4 5)) (newline)         ; (1 2 3 4 5)

;; Test predicates
(write (null? '())) (newline)              ; #t
(write (null? (list 1))) (newline)         ; #f
(write (pair? (cons 1 2))) (newline)       ; #t
(write (pair? '())) (newline)              ; #f
(write (pair? 42)) (newline)               ; #f
