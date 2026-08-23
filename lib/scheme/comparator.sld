;; (scheme comparator) - R7RS-large Red Edition
;;
;; Comparators.
;;
;; R7RS-large names this library `(scheme comparator)`; it is SRFI 128 under its
;; standard-track name. This is a pure re-export of `(srfi 128)` -- the
;; implementation lives there, and the two are the same bindings.
;;
;; That includes SRFI 162's comparator constants and min/max procedures, which
;; SRFI 162 asks implementers to add to their SRFI 128 library rather than ship
;; separately. "Pure re-export" is the whole of `(srfi 128)`, whatever that is,
;; and `r7rs_large_aliases.rs` asserts the two export sets are equal -- so this
;; list cannot quietly fall behind.

(define-library (scheme comparator)
  (import (srfi 128))
  (export
    comparator? comparator-ordered? comparator-hashable? make-comparator make-pair-comparator
    make-list-comparator make-vector-comparator make-eq-comparator make-eqv-comparator
    make-equal-comparator boolean-hash char-hash char-ci-hash string-hash string-ci-hash
    symbol-hash number-hash make-default-comparator default-hash comparator-register-default!
    comparator-type-test-predicate comparator-equality-predicate comparator-ordering-predicate
    comparator-hash-function comparator-test-type comparator-check-type comparator-hash
    hash-bound hash-salt =? <? >? <=? >=? comparator-if<=>
    comparator-max comparator-min comparator-max-in-list comparator-min-in-list
    default-comparator boolean-comparator real-comparator char-comparator
    char-ci-comparator string-comparator string-ci-comparator pair-comparator
    list-comparator vector-comparator eq-comparator eqv-comparator
    equal-comparator))
