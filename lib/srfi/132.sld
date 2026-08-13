;; SRFI 132: Sort Libraries
;;
;; Olin Shivers' sort package with John Cowan's SRFI 132 modifications, from
;; https://github.com/scheme-requests-for-implementation/srfi-132
;; pinned at commit 3e94af40ac389bb9dfd020e3f47728a470d61ec3.
;;
;; License (Shivers): "You may do as you please with this code, as long as you
;; do not delete this notice or hold me responsible for any outcome related to
;; its use." Each file carries its own notice where upstream has one;
;; select.scm is Will Clinger's 2016 addition and has none upstream either.
;;
;; Deviations from upstream (rule: match upstream; mark what cannot match):
;;
;; - This .sld replaces upstream's sorting/132.sld. Upstream splits the
;;   library in two, cond-expands on (rnrs sorting), and imports
;;   (only (srfi 27) random-integer); Patina provides neither R6RS libraries
;;   nor SRFI 27, so this is a single library over the same source files,
;;   with the same (except ...)/(rename ...) import structure upstream uses
;;   to let vector-util.scm shadow vector-copy/vector-copy!. The assert shim
;;   below is copied from upstream's own no-(rnrs base) branch.
;;
;; - select.scm carries the package's only source edit — a (scheme base) RNG
;;   replacing SRFI 27's random-integer — at a ";; PATINA LOCAL EDIT" marker.
;;   Every other included .scm file is byte-identical to upstream
;;   (verified against the pinned commit, 2026-08-12).

(define-library (srfi 132)
  (import (except (scheme base) vector-copy vector-copy!)
          (rename (only (scheme base) vector-copy vector-copy! vector-fill!)
                  (vector-copy  r7rs-vector-copy)
                  (vector-copy! r7rs-vector-copy!)
                  (vector-fill! r7rs-vector-fill!))
          (scheme cxr))

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
    ;; Upstream's assert shim, from 132.sld's no-(rnrs base) branch.
    (define (assert x)
      (if (not x)
          (error "assertion failure"))))

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
