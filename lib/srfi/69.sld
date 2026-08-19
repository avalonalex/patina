;; SRFI 69: Basic Hash Tables
;;
;; Reference implementation from https://srfi.schemers.org/srfi-69/
;; Original author: Panu Kalliokoski
;; License: MIT (see README.md in this directory)

(define-library (srfi 69)
  (import (scheme base)
          (scheme char)
          (scheme case-lambda)
          ;; For `hash-by-identity`, which upstream defines as the structural
          ;; `hash` — see the deviation note in 69/srfi-69-impl.scm.
          (only (patina internal predicates) identity-hash))
  (export make-hash-table hash-table? alist->hash-table
          hash-table-equivalence-function hash-table-hash-function
          hash-table-ref hash-table-ref/default
          hash-table-set! hash-table-delete! hash-table-exists?
          hash-table-update! hash-table-update!/default
          hash-table-size hash-table-keys hash-table-values
          hash-table-walk hash-table-fold
          hash-table->alist hash-table-copy hash-table-merge!
          hash string-hash string-ci-hash hash-by-identity)
  (include "69/srfi-69-impl.scm"))
