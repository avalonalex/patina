(define-library (chrisoei test)

  (import (scheme base)
          (chibi test))

  (export test-approx=)

  (begin
    (define (test-approx= a b tolerance)
      (if (number? a)
      ; if scalar
        (test-assert "approx=" (< (abs (- a b)) tolerance))
      ; else assume a and b are lists
        (begin
          (test "length=" (length a) (length b))
          (map
            (lambda (x y) (test-approx= x y tolerance))
            a
            b
          )
        )
      )
    )
  )
)
