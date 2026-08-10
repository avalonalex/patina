;; SRFI 132: Sort Libraries
;;
;; The reference implementation, unmodified: Olin Shivers' sort package with
;; John Cowan's SRFI 132 modifications, from
;; https://github.com/scheme-requests-for-implementation/srfi-132
;;
;; License (Shivers): "You may do as you please with this code, as long as you
;; do not delete this notice or hold me responsible for any outcome related to
;; its use." Each file carries its own notice; none has been removed.

(define-library (srfi 132)
  (import (scheme base))

  (export
   list-sorted? vector-sorted?
   list-sort list-stable-sort
   list-sort! list-stable-sort!
   vector-sort vector-stable-sort
   vector-sort! vector-stable-sort!
   list-merge list-merge!
   vector-merge vector-merge!
   list-delete-neighbor-dups
   list-delete-neighbor-dups!
   vector-delete-neighbor-dups
   vector-delete-neighbor-dups!
   vector-find-median
   vector-find-median!
   vector-select!
   vector-separate!)

  (begin
    ;; The reference sources call `assert`, which R7RS does not provide.
    (define (assert x)
      (if x x (error "assertion failed"))))

  (include "132/delndups.scm")
  (include "132/lmsort.scm")
  (include "132/sortp.scm")
  (include "132/vector-util.scm")
  (include "132/vhsort.scm")
  (include "132/visort.scm")
  (include "132/vmsort.scm")
  (include "132/vqsort2.scm")
  (include "132/vqsort3.scm")
  (include "132/sort.scm")
  (include "132/select.scm"))
