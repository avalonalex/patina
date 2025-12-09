;; (scheme complex) - R7RS Complex Number Library
;;
;; Complex number operations.

(define-library (scheme complex)
  (import (only (patina internal numbers)
                make-rectangular
                make-polar
                real-part
                imag-part
                magnitude
                angle))

  (export
    make-rectangular
    make-polar
    real-part
    imag-part
    magnitude
    angle))
