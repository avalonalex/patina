;;; DERIV -- Symbolic differentiation.
;;; From ecraven/r7rs-benchmarks, a Gabriel benchmark.
;;;
;;; Tests symbol manipulation, list operations, pattern matching.

(define (deriv a)
  (cond ((not (pair? a))
         (if (eq? a 'x) 1 0))
        ((eq? (car a) '+)
         (cons '+
               (map deriv (cdr a))))
        ((eq? (car a) '-)
         (cons '-
               (map deriv (cdr a))))
        ((eq? (car a) '*)
         (list '*
               a
               (cons '+
                     (map (lambda (a) (list '/ (deriv a) a)) (cdr a)))))
        ((eq? (car a) '/)
         (list '-
               (list '/
                     (deriv (cadr a))
                     (caddr a))
               (list '/
                     (cadr a)
                     (list '*
                           (caddr a)
                           (caddr a)
                           (deriv (caddr a))))))
        (else
         (error "deriv: unknown expression" a))))

;; Standard test expression
(define test-expr
  '(+ (* 3 x x) (* a x x) (* b x) 5))

;; Standard benchmark: 10000000 iterations
;; Smaller for quick testing: 10000 iterations
