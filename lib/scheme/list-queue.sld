;; (scheme list-queue) - R7RS-large Red Edition
;;
;; List queues.
;;
;; R7RS-large names this library `(scheme list-queue)`; it is SRFI 117 under
;; its standard-track name. This is a pure re-export of `(srfi 117)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme list-queue)
  (import (srfi 117))
  (export
    make-list-queue list-queue list-queue-copy list-queue-unfold
    list-queue-unfold-right list-queue? list-queue-empty?
    list-queue-front list-queue-back list-queue-list list-queue-first-last
    list-queue-add-front! list-queue-add-back! list-queue-remove-front!
    list-queue-remove-back! list-queue-remove-all! list-queue-set-list!
    list-queue-append list-queue-append! list-queue-concatenate
    list-queue-map list-queue-map! list-queue-for-each))
