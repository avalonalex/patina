(import (scheme base) (prefix (srfi 11) s:))
(import (patina compat smoke))

(check-equal "fixed multiple-value binding" '(4 5)
  (s:let-values (((a b) (values 4 5))) (list a b)))
(check-equal "parallel initializers see the outer bindings" '(1 99)
  (let ((x 99))
    (s:let-values (((x) (values 1)) ((y) (values x)))
      (list x y))))
(check-equal "sequential initializers see earlier bindings" '(7 8)
  (s:let*-values (((x) (values 7)) ((y) (values (+ x 1))))
    (list x y)))
(check-equal "dotted formals collect remaining values" '(1 (2 3))
  (s:let-values (((first . rest) (values 1 2 3))) (list first rest)))
(check-equal "bare formals collect all values including zero" '((1 2) ())
  (s:let-values ((all (values 1 2)) (none (values))) (list all none)))
(check-equal "empty bindings preserve multiple body values" '(left right)
  (call-with-values
    (lambda () (s:let-values () (values 'left 'right)))
    list))
(check-equal "empty formals accept zero values" 'done
  (s:let-values ((() (values))) 'done))
(define producer-calls 0)
(define produced
  (s:let-values (((a b) (begin
                         (set! producer-calls (+ producer-calls 1))
                         (values 2 3))))
    (+ a b)))
(check-equal "producer is evaluated once" '(5 1)
  (list produced producer-calls))
(check-error "fixed formals reject too few values"
  (s:let-values (((a b) (values 1))) (list a b)))
(smoke-finish)
