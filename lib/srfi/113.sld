;; SRFI 113: Sets and Bags
;;
;; Reference implementation from https://srfi.schemers.org/srfi-113/
;; Original author: John Cowan
;; License: MIT (see README.md in this directory)

(define-library (srfi 113)
  (import (scheme base)
          (scheme case-lambda)
          (scheme write)
          (scheme char)
          (scheme complex)
          (only (scheme r5rs) inexact->exact exact->inexact)
          (srfi 128)
          (srfi 69))

  ;; Sets
  (export set set-unfold)
  (export set? set-contains? set-empty? set-disjoint?)
  (export set-member set-element-comparator)
  (export set-adjoin set-adjoin! set-replace set-replace!
          set-delete set-delete! set-delete-all set-delete-all! set-search!)
  (export set-size set-find set-count set-any? set-every?)
  (export set-map set-for-each set-fold
          set-filter set-remove set-partition
          set-filter! set-remove! set-partition!)
  (export set-copy set->list list->set list->set!)
  (export set=? set<? set>? set<=? set>=?)
  (export set-union set-intersection set-difference set-xor
          set-union! set-intersection! set-difference! set-xor!)
  (export set-comparator)

  ;; Bags
  (export bag bag-unfold)
  (export bag? bag-contains? bag-empty? bag-disjoint?)
  (export bag-member bag-element-comparator)
  (export bag-adjoin bag-adjoin! bag-replace bag-replace!
          bag-delete bag-delete! bag-delete-all bag-delete-all! bag-search!)
  (export bag-size bag-find bag-count bag-any? bag-every?)
  (export bag-map bag-for-each bag-fold
          bag-filter bag-remove bag-partition
          bag-filter! bag-remove! bag-partition!)
  (export bag-copy bag->list list->bag list->bag!)
  (export bag=? bag<? bag>? bag<=? bag>=?)
  (export bag-union bag-intersection bag-difference bag-xor
          bag-union! bag-intersection! bag-difference! bag-xor!)
  (export bag-comparator)

  ;; Bag-specific
  (export bag-sum bag-sum! bag-product bag-product!
          bag-unique-size bag-element-count bag-for-each-unique bag-fold-unique
          bag-increment! bag-decrement! bag->set set->bag set->bag!
          bag->alist alist->bag)

  ;; Comparator shim (SRFI-114 compat helpers used by the impl)
  (include "113/comparators-shim.scm")
  (include "113/sets-impl.scm"))
