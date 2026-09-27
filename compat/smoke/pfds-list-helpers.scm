(import (scheme base) (prefix (pfds list-helpers) l:) (patina compat smoke))
(define original '(1 2 3))
(check-equal "snoc appends without changing the input" '((4) (1 2 3 4) (1 2 3))
  (list (l:snoc '() 4) (l:snoc original 4) original))
(check-equal "take handles zero, exact and oversized lengths"
  '(() (1 2) (1 2 3) (1 2 3))
  (list (l:take original 0) (l:take original 2)
        (l:take original 3) (l:take original 10)))
(check-equal "last returns the final element" '(1 3)
  (list (l:last '(1)) (l:last original)))
(check-equal "but-last removes the final element" '(() (1 2))
  (list (l:but-last '(1)) (l:but-last original)))
(define visited '())
(define mapped
  (l:map-reverse (lambda (x) (set! visited (cons x visited)) (* x x)) original))
(check-equal "map-reverse calls left to right and reverses the results"
  '((9 4 1) (1 2 3) (1 2 3))
  (list mapped (reverse visited) original))
(check-equal "fold-left passes accumulator before element" 4
  (l:fold-left - 10 original))
(check-equal "fold-right associates from the right" -8
  (l:fold-right - 10 original))
(define (unexpected . args) (error "callback should not run" args))
(check-equal "empty traversals preserve their base without callbacks"
  '(left right () ())
  (list (l:fold-left unexpected 'left '()) (l:fold-right unexpected 'right '())
        (l:map-reverse unexpected '()) (l:take '() 3)))
(smoke-finish)
