;; SRFI 128: Comparators (reduced)
;;
;; Reference implementation from https://srfi.schemers.org/srfi-128/
;; Original author: John Cowan
;; License: MIT (see README.md in this directory)
;;
;; SRFI 162's bindings are exported from here rather than from a library of
;; their own, because SRFI 162 says to: "Implementers are urged to add them to
;; their SRFI 128 libraries, for which reason they are not packaged as a
;; separate library." So there is no `(srfi 162)` to bundle — inventing one
;; would name a library the SRFI deliberately declined to define. chibi folds
;; them in the same way, which is why its SRFI 128 suite reaches them through
;; this import and runs here unadapted.
;;
;; `162-impl.scm` is the SRFI's own sample implementation, byte-identical.
;; Note it differs from chibi's copy in one line: chibi comments out
;; `default-comparator` because its `comparators.scm` already defines it, and
;; the SRFI 128 reference implementation this library uses does not.

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
          comparator-if<=>
          ;; SRFI 162 — see the note above on why they live here.
          comparator-max comparator-min
          comparator-max-in-list comparator-min-in-list
          default-comparator boolean-comparator real-comparator
          char-comparator char-ci-comparator
          string-comparator string-ci-comparator
          pair-comparator list-comparator vector-comparator
          eq-comparator eqv-comparator equal-comparator)
  (import (scheme base)
          (scheme case-lambda)
          (scheme char)
          (scheme inexact)
          (scheme complex)
          (only (patina internal predicates) equal-hash))

  (include "128/128.body1.scm")
  (include "128/128.body2.scm")
  (include "128/162-impl.scm"))
