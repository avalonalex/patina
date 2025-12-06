;; Library testing declaration order
;; R7RS: expressions from begin/include processed in order
(define-library (test include-order)
  (export result)
  (import (scheme base))
  (begin (define x 1))
  (include "include-order-set.scm")
  (begin (define result x)))
