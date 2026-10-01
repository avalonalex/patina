(define-library (churnlib)
  (import (scheme base))
  (export x)
  (begin
    (define (churn n acc) (if (> n 0) (churn (- n 1) (cons n (if (pair? acc) (cdr acc) acc))) acc))
    (define x (churn 5000000 '()))))
