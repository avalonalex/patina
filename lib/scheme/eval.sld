;; (scheme eval) - R7RS Eval Library
;;
;; Runtime evaluation.

(define-library (scheme eval)
  (import (patina internal eval))

  (export
    eval
    environment))
