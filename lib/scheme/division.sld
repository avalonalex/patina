;; R7RS-large Tangerine's integer division library, SRFI 141 (#576).
;; Share the existing floor/truncate bindings and the four added families.
(define-library (scheme division)
  (import (srfi 141))
  (export ceiling/ ceiling-quotient ceiling-remainder
          floor/ floor-quotient floor-remainder
          truncate/ truncate-quotient truncate-remainder
          round/ round-quotient round-remainder
          euclidean/ euclidean-quotient euclidean-remainder
          balanced/ balanced-quotient balanced-remainder))
