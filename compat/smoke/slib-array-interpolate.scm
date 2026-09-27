(import (scheme base) (prefix (srfi 63) a:) (prefix (slib array-interpolate) i:)
        (patina compat smoke))
(define (observe thunk) (guard (ex (else 'unexpected-error)) (thunk)))
(define line '#(0 10 20))
(define grid (a:list->array 2 '#() '((1 2 3) (4 5 6))))
(check-equal "integer coordinates select exact elements" '(0 10 20)
  (map (lambda (x) (i:interpolate-array-ref line x)) '(0 1 2)))
(check-equal "one-dimensional interpolation preserves exact fractions" '(5 15/2 15)
  (map (lambda (x) (i:interpolate-array-ref line x)) '(1/2 3/4 3/2)))
(check-equal "one-dimensional coordinates clamp to the endpoints" '(0 20)
  (list (i:interpolate-array-ref line -1/2) (i:interpolate-array-ref line 10)))
(check-equal "bilinear interpolation matches the documented example" 11/4
  (i:interpolate-array-ref grid 1/2 1/4))
(check-equal "integer leading coordinates retain remaining interpolation" 41/10
  (i:interpolate-array-ref grid 1 1/10))
(check-equal "upper multidimensional coordinates clamp before interpolation" 9/2
  (i:interpolate-array-ref grid 10 1/2))
(check-equal "negative leading coordinates clamp before taking a subarray" 3/2
  (observe (lambda () (i:interpolate-array-ref grid -1/2 1/2))))
(check-equal "negative trailing coordinates clamp independently" 5/2
  (i:interpolate-array-ref grid 1/2 -1/2))
(check-equal "clamping also applies to an interior axis of a rank-three array" 13/2
  (observe (lambda ()
    (i:interpolate-array-ref
      (a:list->array 3 '#() '(((1 2) (3 4)) ((11 12) (13 14)))) 1/2 -1/2 1/2))))
(check-equal "rank-zero interpolation returns the scalar" 7
  (i:interpolate-array-ref (a:make-array '#(7))))
(check-equal "singleton axes interpolate to their only value" 7
  (i:interpolate-array-ref '#(7) 9/2))
(check-equal "one-dimensional resampling retains endpoints" '(0 5 10 15 20)
  (let ((dest (a:make-array '#(0) 5)))
    (i:resample-array! dest line) (a:array->list dest)))
(check-equal "resampling a grid interpolates between its corners" '((1 3) (5/2 9/2) (4 6))
  (let ((dest (a:make-array '#(0) 3 2)))
    (i:resample-array! dest grid) (a:array->list dest)))
(check-equal "a singleton destination uses the source origin" '(0)
  (let ((dest (a:make-array '#(99) 1)))
    (i:resample-array! dest line) (a:array->list dest)))
(check-equal "resampling into an empty destination performs no reads" '()
  (let ((dest (vector))) (i:resample-array! dest '#()) (vector->list dest)))
(check-equal "resampling does not mutate its source" '((1 2 3) (4 5 6)) (a:array->list grid))
(smoke-finish)
