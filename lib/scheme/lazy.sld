;; (scheme lazy) - R7RS Lazy Evaluation Library
;;
;; Support for lazy evaluation through promises.

(define-library (scheme lazy)
  (import (patina internal lazy))

  (export
    ;; Public API
    delay
    delay-force
    force
    promise?
    make-promise
    ;; Internal: needed for delay/delay-force macro expansion
    %make-delayed-promise)

  ;; delay and delay-force are macros
  (include "lazy/promises.scm"))
