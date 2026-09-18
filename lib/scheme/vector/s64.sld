;; (scheme vector s64) - R7RS-large Tangerine Edition
;;
;; The full vector API over s64 elements.
;;
;; R7RS-large names this library `(scheme vector s64)`; it is SRFI 160's
;; `(srfi 160 s64)` under its standard-track name. This is a pure re-export
;; -- the implementation lives there, and the two are the same bindings.

(define-library (scheme vector s64)
  (import (srfi 160 s64))
  (export
    make-s64vector s64vector s64vector-unfold s64vector-unfold-right
    s64vector-copy s64vector-reverse-copy s64vector-append
    s64vector-concatenate s64vector-append-subvectors s64? s64vector?
    s64vector-empty? s64vector= s64vector-ref s64vector-length
    s64vector-take s64vector-take-right s64vector-drop
    s64vector-drop-right s64vector-segment s64vector-fold
    s64vector-fold-right s64vector-map s64vector-map! s64vector-for-each
    s64vector-count s64vector-cumulate s64vector-take-while
    s64vector-take-while-right s64vector-drop-while
    s64vector-drop-while-right s64vector-index s64vector-index-right
    s64vector-skip s64vector-skip-right s64vector-any s64vector-every
    s64vector-partition s64vector-filter s64vector-remove s64vector-set!
    s64vector-swap! s64vector-fill! s64vector-reverse! s64vector-copy!
    s64vector-reverse-copy! s64vector-unfold! s64vector-unfold-right!
    s64vector->list list->s64vector reverse-s64vector->list
    reverse-list->s64vector s64vector->vector vector->s64vector
    make-s64vector-generator s64vector-comparator write-s64vector))
