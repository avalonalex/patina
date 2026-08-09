;; (scheme sort) - R7RS-large Red Edition
;;
;; Sort libraries.
;;
;; R7RS-large names this library `(scheme sort)`; it is SRFI 132 under its
;; standard-track name. This is a pure re-export of `(srfi 132)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme sort)
  (import (srfi 132))
  (export
    list-sorted? vector-sorted? list-sort list-stable-sort list-sort! list-stable-sort!
    vector-sort vector-stable-sort vector-sort! vector-stable-sort! list-merge list-merge!
    vector-merge vector-merge! list-delete-neighbor-dups list-delete-neighbor-dups!
    vector-delete-neighbor-dups vector-delete-neighbor-dups! vector-find-median
    vector-find-median! vector-select! vector-separate!))
