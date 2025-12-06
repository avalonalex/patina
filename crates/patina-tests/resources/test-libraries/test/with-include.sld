;; Library with include declaration
(define-library (test with-include)
  (export double-it triple-it)
  (import (scheme base))
  (include "with-include-impl.scm"))
