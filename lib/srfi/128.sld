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
          (scheme complex))

  ;; Provide a portable equal-hash since we don't have SRFI 69/126/R6RS
  (begin
    (define (equal-hash obj)
      (cond
        ((null? obj) 0)
        ((boolean? obj) (if obj 1 0))
        ((number? obj)
         (if (exact? obj)
             (abs (modulo (if (integer? obj) obj (numerator obj)) 536870909))
             (abs (floor (* obj 536870909)))))
        ((char? obj) (char->integer obj))
        ((string? obj)
         (let loop ((i 0) (h 0))
           (if (>= i (string-length obj)) (modulo h 536870909)
               (loop (+ i 1) (+ (* h 31) (char->integer (string-ref obj i)))))))
        ((symbol? obj) (equal-hash (symbol->string obj)))
        ((pair? obj)
         (modulo (+ (* (equal-hash (car obj)) 31) (equal-hash (cdr obj))) 536870909))
        ((vector? obj)
         (let loop ((i 0) (h 0))
           (if (>= i (vector-length obj)) (modulo h 536870909)
               (loop (+ i 1) (+ (* h 31) (equal-hash (vector-ref obj i)))))))
        (else 0))))

  (include "128/128.body1.scm")
  (include "128/128.body2.scm"))
