;; (scheme lseq) - R7RS-large Red Edition
;;
;; Lazy sequences.
;;
;; R7RS-large names this library `(scheme lseq)`; it is SRFI 127 under its
;; standard-track name. This is a pure re-export of `(srfi 127)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme lseq)
  (import (srfi 127))
  (export
    generator->lseq
    lseq? lseq=?
    lseq-car lseq-cdr lseq-first lseq-rest lseq-ref lseq-take lseq-drop
    lseq-realize lseq->generator lseq-length lseq-append lseq-zip
    lseq-map lseq-for-each lseq-filter lseq-remove
    lseq-find lseq-find-tail lseq-any lseq-every lseq-index
    lseq-take-while lseq-drop-while lseq-member lseq-memq lseq-memv))
