;; (scheme inexact) - R7RS Inexact Library
;;
;; Inexact (floating-point) mathematical operations.

(define-library (scheme inexact)
  (import (only (patina internal numbers)
                finite?
                infinite?
                nan?
                exp
                log
                sin
                cos
                tan
                asin
                acos
                atan
                sqrt))

  (export
    finite?
    infinite?
    nan?
    exp
    log
    sin
    cos
    tan
    asin
    acos
    atan
    sqrt))
