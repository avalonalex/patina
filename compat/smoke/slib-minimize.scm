(import (scheme base) (scheme inexact) (slib minimize) (patina compat smoke))
(define (bowl x) (+ 5 (* (- x 2) (- x 2))))
(define (near? x y) (< (abs (- x y)) 1e-5))
(check-equal "positive precision finds an interior quadratic minimum" #t
  (let ((answer (golden-section-search bowl -3 7 1e-7)))
    (and (near? 2 (car answer)) (near? 5 (cdr answer)))))
(check-equal "returned value is the objective at the returned location" #t
  (let ((answer (golden-section-search bowl -3 7 -15)))
    (= (cdr answer) (bowl (car answer)))))
(check-equal "negative precision limits iterations and reuses evaluations" 6
  (let ((calls 0))
    (golden-section-search (lambda (x) (set! calls (+ calls 1)) (bowl x)) -3 7 -5)
    calls))
(check-equal "custom stopping predicate receives an ordered bracket and cached values" '(#t (1 2 3 4))
  (let ((counts '()) (valid #t))
    (golden-section-search bowl -3 7
      (lambda (left right a b fa fb count)
        (set! counts (cons count counts))
        (set! valid (and valid (< left a b right) (= fa (bowl a)) (= fb (bowl b))))
        (= count 4)))
    (list valid (reverse counts))))
(check-equal "a trigonometric minimum is found on a negative interval" #t
  (near? (- (/ (* 4 (atan 1)) 2))
         (car (golden-section-search sin -3 0 1e-7))))
(check-error "reversed bounds are rejected" (golden-section-search bowl 3 1 0.01))
(check-error "equal bounds are rejected" (golden-section-search bowl 2 2 0.01))
(check-error "the objective must be a procedure" (golden-section-search 'bad 0 1 0.01))
(check-error "negative iteration count must be an integer" (golden-section-search bowl 0 4 -1.5))
(check-error "flat objectives fail instead of claiming a unique minimum"
  (golden-section-search (lambda (x) 1) 0 4 -5))
(smoke-finish)
