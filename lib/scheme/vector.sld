;; (scheme vector) - R7RS-large Red Edition
;;
;; Vector library.
;;
;; R7RS-large names this library `(scheme vector)`; it is SRFI 133 under its
;; standard-track name. This is a pure re-export of `(srfi 133)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme vector)
  (import (srfi 133))
  (export
    vector-unfold vector-unfold-right vector-reverse-copy vector-concatenate
    vector-append-subvectors vector-empty? vector= vector-fold vector-fold-right vector-map!
    vector-count vector-cumulate vector-index vector-index-right vector-skip vector-skip-right
    vector-binary-search vector-any vector-every vector-partition vector-swap! vector-reverse!
    vector-reverse-copy! vector-unfold! vector-unfold-right! reverse-vector->list
    reverse-list->vector))
