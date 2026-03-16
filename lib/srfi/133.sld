;; SRFI 133: Vector Library (R7RS)
;;
;; Reference implementation from https://srfi.schemers.org/srfi-133/
;; Original authors: Taylor Campbell (SRFI 43), John Cowan (SRFI 133 modifications)
;; License: MIT (see README.md in this directory)

(define-library (srfi 133)
  (import (scheme base))
  (import (scheme cxr))
  ;; Constructors
  (export vector-unfold vector-unfold-right vector-reverse-copy
          vector-concatenate vector-append-subvectors)
  ;; Predicates
  (export vector-empty? vector=)
  ;; Iteration
  (export vector-fold vector-fold-right vector-map!
          vector-count vector-cumulate)
  ;; Searching
  (export vector-index vector-index-right vector-skip vector-skip-right
          vector-binary-search vector-any vector-every vector-partition)
  ;; Mutators
  (export vector-swap! vector-reverse!
          vector-reverse-copy! vector-unfold! vector-unfold-right!)
  ;; Conversion
  (export reverse-vector->list reverse-list->vector)
  (include "133/vectors-impl.scm"))
