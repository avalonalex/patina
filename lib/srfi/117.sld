;; SRFI 117: mutable list queues.
;;
;; `117/list-queues-impl.scm` is the SRFI's own reference implementation,
;; byte-identical (John Cowan, MIT); see `PROVENANCE.md`. Only this library
;; declaration is local: upstream's names the library `(srfi-117)`, which is
;; not the name R7RS code imports.
(define-library (srfi 117)
  (import (scheme base) (scheme case-lambda))
  (export make-list-queue list-queue list-queue-copy list-queue-unfold
          list-queue-unfold-right
          list-queue? list-queue-empty?
          list-queue-front list-queue-back list-queue-list
          list-queue-first-last
          list-queue-add-front! list-queue-add-back!
          list-queue-remove-front! list-queue-remove-back!
          list-queue-remove-all! list-queue-set-list!
          list-queue-append list-queue-append! list-queue-concatenate
          list-queue-map list-queue-map! list-queue-for-each)
  (include "117/list-queues-impl.scm"))
