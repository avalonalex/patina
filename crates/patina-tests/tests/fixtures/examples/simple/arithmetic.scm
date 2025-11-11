;; Simple arithmetic tests WITHOUT import/display
;; These can be compared directly between Patina and chibi-scheme

(+ 1 2 3)
(- 10 3)
(* 2 3 4)
(/ 20 4)

(= 5 5)
(< 1 2 3)

(cons 1 2)
(car (cons 1 2))
(cdr (cons 1 2))

(null? '())
(pair? (cons 1 2))
