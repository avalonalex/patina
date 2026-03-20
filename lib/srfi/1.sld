;; SRFI 1: List Library
;;
;; Reference implementation from https://srfi.schemers.org/srfi-1/
;; Original author: Olin Shivers
;; License: MIT (see README.md in this directory)
;;
;; Adapted for R7RS: imports (srfi 8) for receive, plus shims for
;; check-arg, let-optionals, :optional.

(define-library (srfi 1)
  (import (scheme base)
          (scheme cxr)
          (srfi 8))

  ;; Constructors
  (export xcons cons* make-list list-tabulate list-copy
          circular-list iota)
  ;; Predicates
  (export proper-list? circular-list? dotted-list?
          not-pair? null-list? list=)
  ;; Selectors
  (export first second third fourth fifth sixth seventh eighth ninth tenth
          car+cdr
          take drop take-right drop-right take! drop-right!
          split-at split-at!
          last last-pair)
  ;; Miscellaneous
  (export length+ zip unzip1 unzip2 unzip3 unzip4 unzip5 count)
  ;; Append and concatenate
  (export append! append-reverse append-reverse!
          concatenate concatenate!)
  ;; Fold, unfold & map
  (export fold unfold pair-fold reduce
          fold-right unfold-right pair-fold-right reduce-right
          append-map append-map! map! pair-for-each filter-map map-in-order)
  ;; Filtering & partitioning
  (export filter partition remove
          filter! partition! remove!)
  ;; Searching
  (export find find-tail any every list-index
          take-while drop-while take-while!
          span break span! break!)
  ;; Deleting
  (export delete delete!
          delete-duplicates delete-duplicates!)
  ;; Association lists
  (export alist-cons alist-copy
          alist-delete alist-delete!)
  ;; Set operations on lists
  (export lset<= lset= lset-adjoin
          lset-union lset-intersection lset-difference lset-xor
          lset-diff+intersection
          lset-union! lset-intersection! lset-difference! lset-xor!
          lset-diff+intersection!)
  ;; Mutation
  (export reverse!)

  ;; R7RS compatibility macros must be defined in the .sld before the
  ;; reference implementation include, so they are available during
  ;; macro expansion of the included code.

  ;; :optional — extract an optional argument from a rest list
  (begin
    (define-syntax :optional
      (syntax-rules ()
        ((:optional rest default-exp)
         (if (null? rest) default-exp (car rest)))
        ((:optional rest default-exp pred)
         (let ((val (if (null? rest) default-exp (car rest))))
           (if (pred val) val
               (error "Bad optional argument" val)))))))

  ;; let-optionals — bind optional arguments from a rest list
  (begin
    (define-syntax let-optionals
      (syntax-rules ()
        ((let-optionals rest () body ...)
         (begin body ...))
        ((let-optionals rest ((var default) . more) body ...)
         (let ((var (if (null? rest) default (car rest)))
               (rest* (if (null? rest) '() (cdr rest))))
           (let-optionals rest* more body ...)))
        ((let-optionals rest (var . more) body ...)
         (let ((var (car rest))
               (rest* (cdr rest)))
           (let-optionals rest* more body ...))))))

  ;; check-arg: parameter validation
  (begin
    (define (check-arg pred val caller)
      (if (pred val)
          val
          (error "Bad argument" val caller))))

  ;; Reference implementation
  (include "1/srfi-1-reference.scm"))
