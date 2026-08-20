;;; The sort package -- general sort & merge procedures
;;;
;;; Copyright (c) 1998 by Olin Shivers.
;;; You may do as you please with this code, as long as you do not delete this
;;; notice or hold me responsible for any outcome related to its use.
;;; Olin Shivers 10/98.

;;; This file just defines the general sort API in terms of some
;;; algorithm-specific calls.

;; PATINA LOCAL EDIT: upstream sorts via vector-heap-sort!, which reverses
;; ties. SRFI 132 allows that — list-sort need not be stable — but both
;; references ship a stable list-sort (chibi's delegates to its native sort
;; rather than to this reference; so does Gauche's), and real code depends
;; on it: chibi-voting's sort-pairs breaks residual ties by input order.
;; vector-sort below deliberately keeps upstream's unstable quicksort: the
;; both-references argument does not extend there (chibi's vector-sort is
;; stable, but Gauche routes vector-sort to its unstable sort!), and no
;; corpus package implicates it. Deviation recorded in the 132.sld header.
(define list-sort list-merge-sort)

(define list-sort! list-merge-sort!)

(define list-stable-sort  list-merge-sort)
(define list-stable-sort! list-merge-sort!)

(define vector-sort  vector-quick-sort)
(define vector-sort! vector-quick-sort!)

(define vector-stable-sort  vector-merge-sort)
(define vector-stable-sort! vector-merge-sort!)

