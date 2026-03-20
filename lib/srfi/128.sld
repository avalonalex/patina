;; SRFI 128: Comparators (reduced)
;;
;; Reference implementation from https://srfi.schemers.org/srfi-128/
;; Original author: John Cowan
;; License: MIT (see README.md in this directory)

(define-library (srfi 128)
  (export comparator? comparator-ordered? comparator-hashable?
          make-comparator
          make-pair-comparator make-list-comparator make-vector-comparator
          make-eq-comparator make-eqv-comparator make-equal-comparator
          boolean-hash char-hash char-ci-hash
          string-hash string-ci-hash symbol-hash number-hash
          make-default-comparator default-hash comparator-register-default!
          comparator-type-test-predicate comparator-equality-predicate
          comparator-ordering-predicate comparator-hash-function
          comparator-test-type comparator-check-type comparator-hash
          hash-bound hash-salt
          =? <? >? <=? >=?
          comparator-if<=>)
  (import (scheme base)
          (scheme case-lambda)
          (scheme char)
          (scheme inexact)
          (scheme complex)
          (only (patina internal predicates) equal-hash))

  (include "128/128.body1.scm")
  (include "128/128.body2.scm"))
