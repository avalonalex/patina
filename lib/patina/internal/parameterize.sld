;; (patina internal parameterize) - `parameterize`'s loop for a binding that
;; is not a parameter object, loaded on first use (#817). See
;; parameterize.scm.

(define-library (patina internal parameterize)
  (import (scheme base))
  (export %parameterize-each)
  (include "parameterize.scm"))
