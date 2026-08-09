;; (scheme list) - R7RS-large Red Edition
;;
;; List library.
;;
;; R7RS-large names this library `(scheme list)`; it is SRFI 1 under its
;; standard-track name. This is a pure re-export of `(srfi 1)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme list)
  (import (srfi 1))
  (export
    xcons cons* make-list list-tabulate list-copy circular-list iota proper-list?
    circular-list? dotted-list? not-pair? null-list? list= first second third fourth fifth
    sixth seventh eighth ninth tenth car+cdr take drop take-right drop-right take! drop-right!
    split-at split-at! last last-pair length+ zip unzip1 unzip2 unzip3 unzip4 unzip5 count
    append! append-reverse append-reverse! concatenate concatenate! fold unfold pair-fold
    reduce fold-right unfold-right pair-fold-right reduce-right append-map append-map! map!
    pair-for-each filter-map map-in-order filter partition remove filter! partition! remove!
    find find-tail any every list-index take-while drop-while take-while! span break span!
    break! delete delete! delete-duplicates delete-duplicates! alist-cons alist-copy
    alist-delete alist-delete! lset<= lset= lset-adjoin lset-union lset-intersection
    lset-difference lset-xor lset-diff+intersection lset-union! lset-intersection!
    lset-difference! lset-xor! lset-diff+intersection! reverse!))
