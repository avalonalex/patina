;; (scheme vector c128) - R7RS-large Tangerine Edition
;;
;; The full vector API over c128 elements.
;;
;; R7RS-large names this library `(scheme vector c128)`; it is SRFI 160's
;; `(srfi 160 c128)` under its standard-track name. This is a pure re-export
;; -- the implementation lives there, and the two are the same bindings.

(define-library (scheme vector c128)
  (import (srfi 160 c128))
  (export
    make-c128vector c128vector c128vector-unfold c128vector-unfold-right
    c128vector-copy c128vector-reverse-copy c128vector-append
    c128vector-concatenate c128vector-append-subvectors c128? c128vector?
    c128vector-empty? c128vector= c128vector-ref c128vector-length
    c128vector-take c128vector-take-right c128vector-drop
    c128vector-drop-right c128vector-segment c128vector-fold
    c128vector-fold-right c128vector-map c128vector-map!
    c128vector-for-each c128vector-count c128vector-cumulate
    c128vector-take-while c128vector-take-while-right
    c128vector-drop-while c128vector-drop-while-right c128vector-index
    c128vector-index-right c128vector-skip c128vector-skip-right
    c128vector-any c128vector-every c128vector-partition c128vector-filter
    c128vector-remove c128vector-set! c128vector-swap! c128vector-fill!
    c128vector-reverse! c128vector-copy! c128vector-reverse-copy!
    c128vector-unfold! c128vector-unfold-right! c128vector->list
    list->c128vector reverse-c128vector->list reverse-list->c128vector
    c128vector->vector vector->c128vector make-c128vector-generator
    c128vector-comparator write-c128vector))
