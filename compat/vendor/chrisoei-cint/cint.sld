(define-library (chrisoei cint)

  (import (scheme base))

  (export cint)

  (begin

    (define (normalize v)
      (let
        (
          ; s is the sum of all the elements of vector v
          (s (apply + v))
        )
        ; divide each element of v by s
        (map
          (lambda (x)
            (/ x s)
          )
          v
        )
      )
    )

    (define (cint o l)
      (let*
        (
          (p (expt 2 (- o 1)))
        )
        ; recur is a private function
        (define (recur i f x)
          (if (= i 0)
            '()
            (cons
              x
              (recur
                (- i 1)
                (* 2 f)
                (/ x p (- 1 f))
              )
            )
          )
        )
        (normalize (recur l 2 1.0))
      )
    )

  )
)

; vim: set et ff=unix ft=scheme nocp sts=2 sw=2 ts=2:
