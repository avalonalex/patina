(import (scheme base) (scheme write) (patina debug))
(define (churn n acc) (if (> n 0) (churn (- n 1) (cons n (if (pair? acc) (cdr acc) acc))) acc))
(define x (churn 5000000 '()))
(display (assq 'collections (gc-stats))) (display (assq 'pairs (gc-stats))) (newline)
