;; Library with multiple includes (SRFI-1 pattern)
(define-library (test multi-include)
  (export add sub mul proper-list?)
  (import (scheme base))
  (include "multi-include/arithmetic.scm"
           "multi-include/predicates.scm"))
