(import (scheme base) (scheme write) (patina debug))
(define (churn n acc) (if (> n 0) (churn (- n 1) (cons n (if (pair? acc) (cdr acc) acc))) acc))
(churn 10000000 '())
(display (gc-stats)) (newline)
