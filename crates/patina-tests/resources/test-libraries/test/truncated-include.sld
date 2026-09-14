;; A library whose included implementation file is cut short.
(define-library (test truncated-include)
  (import (scheme base))
  (export half)
  (include "truncated-include-impl.scm"))
