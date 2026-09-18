;; (scheme vector u16) - R7RS-large Tangerine Edition
;;
;; The full vector API over u16 elements.
;;
;; R7RS-large names this library `(scheme vector u16)`; it is SRFI 160's
;; `(srfi 160 u16)` under its standard-track name. This is a pure re-export
;; -- the implementation lives there, and the two are the same bindings.

(define-library (scheme vector u16)
  (import (srfi 160 u16))
  (export
    make-u16vector u16vector u16vector-unfold u16vector-unfold-right
    u16vector-copy u16vector-reverse-copy u16vector-append
    u16vector-concatenate u16vector-append-subvectors u16? u16vector?
    u16vector-empty? u16vector= u16vector-ref u16vector-length
    u16vector-take u16vector-take-right u16vector-drop
    u16vector-drop-right u16vector-segment u16vector-fold
    u16vector-fold-right u16vector-map u16vector-map! u16vector-for-each
    u16vector-count u16vector-cumulate u16vector-take-while
    u16vector-take-while-right u16vector-drop-while
    u16vector-drop-while-right u16vector-index u16vector-index-right
    u16vector-skip u16vector-skip-right u16vector-any u16vector-every
    u16vector-partition u16vector-filter u16vector-remove u16vector-set!
    u16vector-swap! u16vector-fill! u16vector-reverse! u16vector-copy!
    u16vector-reverse-copy! u16vector-unfold! u16vector-unfold-right!
    u16vector->list list->u16vector reverse-u16vector->list
    reverse-list->u16vector u16vector->vector vector->u16vector
    make-u16vector-generator u16vector-comparator write-u16vector))
