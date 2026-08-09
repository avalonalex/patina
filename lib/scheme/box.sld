;; (scheme box) - R7RS-large Red Edition
;;
;; Boxes (single-value containers).
;;
;; R7RS-large names this library `(scheme box)`; it is SRFI 111 under its
;; standard-track name. This is a pure re-export of `(srfi 111)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme box)
  (import (srfi 111))
  (export
    box box? unbox set-box!))
