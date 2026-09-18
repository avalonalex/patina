;; SRFI 39: parameter objects.
;;
;; A re-export, not an implementation. Both became R7RS-small 4.2.6, so
;; `(scheme base)` already has them.
;;
;; One deviation is upstream's, not ours, and is worth knowing: SRFI 39's
;; `parameterize` requires the converter to be applied to the new value, and
;; R7RS says the same, so the two agree — but SRFI 39 leaves the *order* of
;; converter application across several bindings unspecified where R7RS does
;; too. Nothing here narrows either.
;;
;; Bundled for the reason recorded in lib/srfi/PROVENANCE.md.

(define-library (srfi 39)
  (import (only (scheme base) make-parameter parameterize))
  (export make-parameter parameterize))
