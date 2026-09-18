;; SRFI 16: syntax for procedures of variable arity.
;;
;; A re-export, not an implementation. `case-lambda` became R7RS-small 4.2.9,
;; and `lib/scheme/case-lambda.sld` is already SRFI 16's own reference
;; implementation — so the two are the same macro, and giving this library its
;; own copy would be two definitions to keep in step for no gain.
;;
;; Bundled for `(srfi 146)`, whose Gleckler HAMT libraries import it — see
;; lib/srfi/PROVENANCE.md.

(define-library (srfi 16)
  (import (scheme case-lambda))
  (export case-lambda))
