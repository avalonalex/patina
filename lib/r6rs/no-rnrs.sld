;;; A marker library, empty by design.
;;;
;;; Clinger's libraries guard every branch with
;;;
;;;     (and (or (library (rnrs base)) larceny)
;;;          (not (library (r6rs no-rnrs))))
;;;
;;; so that on a host which already *has* `(rnrs ...)` — a real R6RS
;;; implementation — `(r6rs base)` becomes a re-export of it rather than a
;;; second copy. Patina has `(rnrs ...)` only because `lib/rnrs/` re-exports
;;; these very libraries, so that branch would close a cycle: `(rnrs base)`
;;; imports `(r6rs base)` imports `(rnrs base)`.
;;;
;;; Defining this library is upstream's own way of saying "the `(rnrs ...)` you
;;; can see is not a host implementation", which sends every guard to its
;;; `else` branch — the portable R7RS implementation, which is the one Patina
;;; wants. It is the reason `lib/r6rs/` needs no local edit at all.

(define-library (r6rs no-rnrs)
  (import (scheme base))
  (export))
