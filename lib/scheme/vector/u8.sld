;; (scheme vector u8) - R7RS-large Tangerine Edition
;;
;; The full vector API over u8 elements.
;;
;; R7RS-large names this library `(scheme vector u8)`; it is SRFI 160's
;; `(srfi 160 u8)` under its standard-track name. This is a pure re-export
;; -- the implementation lives there, and the two are the same bindings.

(define-library (scheme vector u8)
  (import (srfi 160 u8))
  (export
    make-u8vector u8vector u8vector-unfold u8vector-unfold-right
    u8vector-copy u8vector-reverse-copy u8vector-append
    u8vector-concatenate u8vector-append-subvectors u8? u8vector?
    u8vector-empty? u8vector= u8vector-ref u8vector-length u8vector-take
    u8vector-take-right u8vector-drop u8vector-drop-right u8vector-segment
    u8vector-fold u8vector-fold-right u8vector-map u8vector-map!
    u8vector-for-each u8vector-count u8vector-cumulate u8vector-take-while
    u8vector-take-while-right u8vector-drop-while
    u8vector-drop-while-right u8vector-index u8vector-index-right
    u8vector-skip u8vector-skip-right u8vector-any u8vector-every
    u8vector-partition u8vector-filter u8vector-remove u8vector-set!
    u8vector-swap! u8vector-fill! u8vector-reverse! u8vector-copy!
    u8vector-reverse-copy! u8vector-unfold! u8vector-unfold-right!
    u8vector->list list->u8vector reverse-u8vector->list
    reverse-list->u8vector u8vector->vector vector->u8vector
    make-u8vector-generator u8vector-comparator write-u8vector))
