;; (scheme vector u64) - R7RS-large Tangerine Edition
;;
;; The full vector API over u64 elements.
;;
;; R7RS-large names this library `(scheme vector u64)`; it is SRFI 160's
;; `(srfi 160 u64)` under its standard-track name. This is a pure re-export
;; -- the implementation lives there, and the two are the same bindings.

(define-library (scheme vector u64)
  (import (srfi 160 u64))
  (export
    make-u64vector u64vector u64vector-unfold u64vector-unfold-right
    u64vector-copy u64vector-reverse-copy u64vector-append
    u64vector-concatenate u64vector-append-subvectors u64? u64vector?
    u64vector-empty? u64vector= u64vector-ref u64vector-length
    u64vector-take u64vector-take-right u64vector-drop
    u64vector-drop-right u64vector-segment u64vector-fold
    u64vector-fold-right u64vector-map u64vector-map! u64vector-for-each
    u64vector-count u64vector-cumulate u64vector-take-while
    u64vector-take-while-right u64vector-drop-while
    u64vector-drop-while-right u64vector-index u64vector-index-right
    u64vector-skip u64vector-skip-right u64vector-any u64vector-every
    u64vector-partition u64vector-filter u64vector-remove u64vector-set!
    u64vector-swap! u64vector-fill! u64vector-reverse! u64vector-copy!
    u64vector-reverse-copy! u64vector-unfold! u64vector-unfold-right!
    u64vector->list list->u64vector reverse-u64vector->list
    reverse-list->u64vector u64vector->vector vector->u64vector
    make-u64vector-generator u64vector-comparator write-u64vector))
