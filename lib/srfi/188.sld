;; SRFI 188: splicing binding constructs for syntactic keywords.
;; Original Patina implementation in the shared frontend (#424). These forms
;; need expander support: ordinary let-syntax cannot export its definitions.
;; Specification: https://srfi.schemers.org/srfi-188/srfi-188.html (final 2020-06-03).
(define-library (srfi 188)
  (import (only (patina internal syntax)
                splicing-let-syntax splicing-letrec-syntax))
  (export splicing-let-syntax splicing-letrec-syntax))
