;; SRFI 125: Intermediate Hash Tables
;;
;; `125/hash.scm` is byte-identical to chibi-scheme 0.12.0's
;; `lib/srfi/125/hash.scm` (Alex Shinn, BSD 3-Clause); provenance and the
;; licence text are in `lib/srfi/PROVENANCE.md`. It is a thin layer over
;; SRFI 69 and SRFI 128 rather than a hash table of its own, which is why
;; bundling it needs no new runtime support — `equal-hash` is already a
;; primitive and `(srfi 69)` is already a real bucket-vector table.
;;
;; Two clauses below stand in for things upstream takes from chibi. Both are
;; local to this file; `125/hash.scm` itself is untouched.
;;
;; 1. **Hash-function arity.** SRFI 69 hash functions take `(obj [bound])` and
;;    return a value below `bound`; SRFI 128 comparator hash functions take
;;    `(obj)` and return any non-negative integer. `hash.scm` hands the latter
;;    straight to SRFI 69's constructors, which works on chibi because its
;;    SRFI 69 is C-backed and reduces the value itself. Ours is the portable
;;    reference implementation, which calls `(hash key size)` — so a
;;    comparator's hash function would be called with one argument too many.
;;    `%make-hash-table` and `%alist->hash-table` adapt it here.
;;
;; 2. **Immutability.** `hash.scm` uses `(chibi ast)`'s `immutable?` and
;;    `make-immutable!` to implement `hash-table-mutable?` and the immutable
;;    result of a one-argument `hash-table-copy`. Patina has no object-level
;;    immutability, so the flag lives in a table keyed by identity. The cost is
;;    that a table marked immutable is remembered for the life of the program;
;;    only `hash-table-copy` with a false or absent second argument creates
;;    one, so the set stays small.
(define-library (srfi 125)
  (import (scheme base)
          ;; SRFI 128 also binds `string-hash` and `string-ci-hash`, but to the
          ;; one-argument comparator convention. SRFI 125 re-exports SRFI 69's,
          ;; which take an optional bound, so 128's are excluded rather than
          ;; left to collide.
          (except (srfi 128) string-hash string-ci-hash)
          (rename (srfi 69)
                  (make-hash-table srfi-69:make-hash-table)
                  (alist->hash-table srfi-69:alist->hash-table)
                  (hash-table-copy %hash-table-copy)
                  (hash-table-set! %hash-table-set!)
                  (hash-table-delete! %hash-table-delete!)
                  (hash-table-fold %hash-table-fold)
                  (hash-table-ref srfi-69:hash-table-ref)
                  (hash-table-merge! srfi-69:hash-table-merge!)))
  (export
   ;; Constructors:
   make-hash-table hash-table hash-table-unfold alist->hash-table
   ;; Predicates:
   hash-table? hash-table-contains? hash-table-exists?
   hash-table-empty? hash-table=? hash-table-mutable?
   ;; Accessors:
   hash-table-ref hash-table-ref/default
   ;; Mutators:
   hash-table-set! hash-table-delete! hash-table-intern!
   hash-table-update! hash-table-update!/default hash-table-pop!
   hash-table-clear!
   ;; The whole hash table:
   hash-table-size hash-table-keys hash-table-values
   hash-table-entries hash-table-find hash-table-count
   ;; Mapping and folding:
   hash-table-map hash-table-for-each hash-table-walk
   hash-table-map! hash-table-map->list hash-table-fold hash-table-prune!
   ;; Copying and conversion:
   hash-table-copy hash-table-empty-copy hash-table->alist
   ;; Hash tables as sets:
   hash-table-union! hash-table-merge!
   hash-table-intersection! hash-table-difference! hash-table-xor!
   ;; Hash functions and reflectivity:
   hash string-hash string-ci-hash hash-by-identity
   hash-table-equivalence-function hash-table-hash-function)

  (begin
    ;; Deviation 1: give a SRFI 128 hash function SRFI 69's calling convention.
    (define (as-srfi-69-hash-function hash)
      (lambda (obj . bound)
        (let ((h (hash obj)))
          (if (pair? bound) (modulo h (car bound)) h))))

    (define (%make-hash-table equal hash)
      (srfi-69:make-hash-table equal (as-srfi-69-hash-function hash)))

    (define (%alist->hash-table alist equal hash)
      (srfi-69:alist->hash-table alist equal (as-srfi-69-hash-function hash)))

    ;; Deviation 2: immutability, tracked beside the tables rather than in them.
    (define immutable-tables (srfi-69:make-hash-table eq?))

    (define (immutable? ht)
      (hash-table-ref/default immutable-tables ht #f))

    (define (make-immutable! ht)
      (%hash-table-set! immutable-tables ht #t)
      ht)

    ;; Deviation 3: two procedures SRFI 125 redefines rather than inherits.
    ;; `hash.scm` re-exports SRFI 69's, which is right on chibi because its
    ;; SRFI 69 is C-backed and already has the wider behaviour; the portable
    ;; reference implementation we bundle has the SRFI 69 behaviour only.
    ;;
    ;; `hash-table-ref` gains SRFI 125's third argument: SRFI 69 takes only a
    ;; failure thunk, where SRFI 125 also applies `success` to the value found.
    (define missing (list 'missing))

    (define (hash-table-ref ht key . failure+success)
      (let ((value (hash-table-ref/default ht key missing)))
        (cond
         ((eq? value missing)
          (if (pair? failure+success)
              ((car failure+success))
              (error "hash-table-ref: no association for key" key)))
         ((and (pair? failure+success) (pair? (cdr failure+success)))
          ((cadr failure+success) value))
         (else value))))

    ;; `hash-table-union!` (spelled `merge!` upstream, which `hash.scm`
    ;; aliases) must leave associations already in `ht1` alone. SRFI 69's
    ;; overwrites them, which silently gave `ht2` priority — the opposite of
    ;; what SRFI 125 specifies.
    (define (hash-table-merge! ht1 ht2)
      (hash-table-walk
       ht2
       (lambda (key value)
         (if (not (hash-table-exists? ht1 key))
             (%hash-table-set! ht1 key value))))
      ht1))

  (include "125/hash.scm"))
