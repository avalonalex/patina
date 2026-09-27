(import (scheme base) (prefix (srfi 16) s:))
(import (patina compat smoke))

(define dispatch
  (s:case-lambda
    (() 'empty)
    ((x) (list 'one x))
    ((x y) (list 'two x y))))
(check-equal "dispatch chooses the matching fixed arity"
  '(empty (one 7) (two 7 8))
  (list (dispatch) (dispatch 7) (dispatch 7 8)))
(define dotted (s:case-lambda ((first second . rest) (list first second rest))))
(check-equal "dotted formals accept empty or nonempty rest"
  '((1 2 ()) (1 2 (3 4)))
  (list (dotted 1 2) (dotted 1 2 3 4)))
(define any (s:case-lambda (args args)))
(check-equal "bare formals collect any number of arguments" '(() (a b))
  (list (any) (any 'a 'b)))
(check-equal "the first matching clause wins" 'rest
  ((s:case-lambda ((x . rest) 'rest) ((x) 'fixed)) 1))
(check-equal "multiple body values reach the caller" '(3 4)
  (call-with-values
    (lambda () ((s:case-lambda ((x) (values x (+ x 1)))) 3))
    list))
(check-equal "internal names do not capture user bindings" '(3 outside length)
  (let ((args 'outside) (l 'length))
    ((s:case-lambda ((x) (list x args l))) 3)))
(check-error "unmatched arity raises" (dispatch 1 2 3))
(check-error "a procedure with no clauses raises when called" ((s:case-lambda)))
(smoke-finish)
