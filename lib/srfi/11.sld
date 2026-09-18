;; SRFI 11: syntax for receiving multiple values.
;;
;; A re-export, not an implementation. Both forms became R7RS-small 4.2.2, so
;; `(scheme base)` already has them.
;;
;; Note what this library is *not*: SRFI 8's `receive`, which is a different
;; SRFI and is bundled separately at `lib/srfi/8.sld`.
;;
;; Bundled for the reason recorded in lib/srfi/PROVENANCE.md.

(define-library (srfi 11)
  (import (only (scheme base) let-values let*-values))
  (export let-values let*-values))
