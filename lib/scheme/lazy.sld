;; (scheme lazy) - R7RS Lazy Evaluation Library
;;
;; Support for lazy evaluation through promises.

(define-library (scheme lazy)
  ;; (patina internal syntax) for `define-syntax`, `syntax-rules` and `lambda`,
  ;; which lazy/promises.scm uses. A library gets syntactic keywords only by
  ;; importing them, exactly as R7RS requires and as chibi and Gauche enforce;
  ;; before stage 2 the desugarer recognized them by spelling in every scope.
  ;; Imported directly rather than via (scheme base) to keep the dependency at
  ;; what this library actually uses.
  (import (patina internal syntax)
          (patina internal lazy))

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
