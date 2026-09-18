;; (scheme vector s32) - R7RS-large Tangerine Edition
;;
;; The full vector API over s32 elements.
;;
;; R7RS-large names this library `(scheme vector s32)`; it is SRFI 160's
;; `(srfi 160 s32)` under its standard-track name. This is a pure re-export
;; -- the implementation lives there, and the two are the same bindings.

(define-library (scheme vector s32)
  (import (srfi 160 s32))
  (export
    make-s32vector s32vector s32vector-unfold s32vector-unfold-right
    s32vector-copy s32vector-reverse-copy s32vector-append
    s32vector-concatenate s32vector-append-subvectors s32? s32vector?
    s32vector-empty? s32vector= s32vector-ref s32vector-length
    s32vector-take s32vector-take-right s32vector-drop
    s32vector-drop-right s32vector-segment s32vector-fold
    s32vector-fold-right s32vector-map s32vector-map! s32vector-for-each
    s32vector-count s32vector-cumulate s32vector-take-while
    s32vector-take-while-right s32vector-drop-while
    s32vector-drop-while-right s32vector-index s32vector-index-right
    s32vector-skip s32vector-skip-right s32vector-any s32vector-every
    s32vector-partition s32vector-filter s32vector-remove s32vector-set!
    s32vector-swap! s32vector-fill! s32vector-reverse! s32vector-copy!
    s32vector-reverse-copy! s32vector-unfold! s32vector-unfold-right!
    s32vector->list list->s32vector reverse-s32vector->list
    reverse-list->s32vector s32vector->vector vector->s32vector
    make-s32vector-generator s32vector-comparator write-s32vector))
