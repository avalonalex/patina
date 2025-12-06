;; Predicate operations
(define (proper-list? x)
  (or (null? x)
      (and (pair? x)
           (proper-list? (cdr x)))))
