;; (scheme ephemeron) - R7RS-large Red Edition
;;
;; Ephemerons.
;;
;; R7RS-large names this library `(scheme ephemeron)`; it is SRFI 124 under its
;; standard-track name. This is a pure re-export of `(srfi 124)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme ephemeron)
  (import (srfi 124))
  (export make-ephemeron ephemeron? ephemeron-broken?
          ephemeron-key ephemeron-datum reference-barrier))
