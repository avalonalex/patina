;; SRFI 125: Intermediate Hash Tables
;;
;; A thin layer over SRFI 69 and SRFI 128 rather than a hash table of its own.
;; `125/hash.scm` is byte-identical to upstream; provenance and licence are in
;; `lib/srfi/PROVENANCE.md`.
;;
;; Four deviations, all in the `(begin ...)` below — `125/hash.scm` itself is
;; untouched. Each exists because upstream runs against chibi's *C-backed*
;; SRFI 69, which already has behaviour the portable reference implementation
;; we bundle does not, so upstream inherits what we have to supply.
;;
;; 1. **Hash-function arity.** SRFI 69 hash functions take `(obj [bound])` and
;;    return a value below `bound`; SRFI 128 comparator hash functions take
;;    `(obj)` and return anything. Our SRFI 69 always passes the bound, so a
;;    comparator's would be called with one argument too many. Every hash
;;    function reaching SRFI 69's constructors is therefore adapted, and called
;;    with one argument.
;;
;;    That placement is a trade, not an oversight, and neither side is free:
;;    adapting *only* the comparator path would leave a caller who extracts
;;    `(comparator-hash-function c)` by hand and passes it along — which the
;;    upstream conformance suite does — handing SRFI 69 a one-argument
;;    procedure. Adapting everything instead costs the reverse: a hash function
;;    that *requires* two arguments works under plain `(srfi 69)`, which always
;;    supplies the bound, and fails here. Adapting everything is the better
;;    trade because every hash function SRFI 69 itself defines takes the bound
;;    optionally, so a conforming one is always callable with one argument;
;;    only a two-argument-only procedure, which SRFI 125 never produces, is
;;    lost. Distinguishing them needs arity introspection Scheme does not have.
;;
;;    `lib/srfi/113/sets-impl.scm`'s `modulizer` is the same shim for the same
;;    mismatch, inside a vendored include.
;;
;; 2. **Immutability.** Upstream uses `(chibi ast)`'s `immutable?` and
;;    `make-immutable!` for `hash-table-mutable?` and the immutable result of a
;;    one-argument `hash-table-copy`. Patina has no object-level immutability,
;;    so the flag lives in a table keyed by identity. Two consequences worth
;;    stating rather than leaving to be discovered:
;;      - the flag is **advisory**. SRFI 69's `hash-table-set!` cannot consult
;;        it, so mutating a table that reports `(hash-table-mutable? ht)` =>
;;        `#f` still succeeds. chibi's object-level mark is enforced; this is
;;        not.
;;      - entries are **never released**, so an immutable copy is pinned for
;;        the life of the process — and the leaky spelling is the default one,
;;        `(hash-table-copy ht)`. Bounding it needs weak references keyed by
;;        object identity; the runtime has that machinery for the VM's
;;        continuation tables (`GcRoots::trace_weak_ids` / `sweep_weak`, see
;;        `docs/GC_DESIGN.md`) but does not expose it to Scheme.
;;
;; 3. **`hash-table-ref` and `hash-table-update!`** take SRFI 125's `success`
;;    argument, which SRFI 69's do not; inherited, `success` was accepted and
;;    silently ignored.
;;
;; 4. **`hash-table-union!`** (spelled `merge!` upstream, which `hash.scm`
;;    aliases) must leave associations already in the first table alone.
;;    SRFI 69's overwrites them, which silently gave the *second* table
;;    priority — the opposite of what SRFI 125 specifies, and invisible
;;    because both tables still end up the right size.
;;
;; Widening `(srfi 69)` itself was considered and rejected for 3 and 4: it is a
;; separate published SRFI with its own narrower spec and 16 importers in the
;; compat corpus, so changing it would alter results for code that never asked
;; for SRFI 125.
;;
;; One thing upstream does that this list does not change, recorded because it
;; is a silent one: `(make-hash-table comparator [weakness])` drops the
;; weakness argument. SRFI 125 says an error should be signalled for a
;; weakness the implementation does not support, and Patina supports none —
;; `hash.scm`'s `make-hash-table` reads `o` only for a hash function. Left
;; matching upstream rather than fixed, because signalling would reject code
;; that runs on chibi today, and because the argument asks for a memory
;; property rather than a behavioural one: a table that holds its keys
;; strongly is a conforming table that leaks, not a wrong answer (audit F2).
(define-library (srfi 125)
  (import (scheme base)
          ;; SRFI 128 also binds `string-hash` and `string-ci-hash`, to the
          ;; one-argument convention; SRFI 125 re-exports SRFI 69's, which take
          ;; an optional bound, so 128's are excluded rather than left to
          ;; collide.
          (except (srfi 128) string-hash string-ci-hash)
          ;; The three names deviations 3 and 4 replace are excluded; the two
          ;; constructors are renamed so deviation 1 can wrap what they are
          ;; handed. The rest is upstream's own rename list.
          (rename (except (srfi 69)
                          hash-table-ref hash-table-update! hash-table-merge!)
                  (make-hash-table srfi-69:make-hash-table)
                  (alist->hash-table srfi-69:alist->hash-table)
                  (hash-table-copy %hash-table-copy)
                  (hash-table-set! %hash-table-set!)
                  (hash-table-delete! %hash-table-delete!)
                  (hash-table-fold %hash-table-fold))
          (only (patina internal predicates) equal-hash))
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
    ;; Deviation 1.
    (define (as-srfi-69-hash-function hash)
      (lambda (obj . bound)
        (let ((h (hash obj)))
          (if (pair? bound) (modulo h (car bound)) h))))

    (define (%make-hash-table equal hash)
      (srfi-69:make-hash-table equal (as-srfi-69-hash-function hash)))

    (define (%alist->hash-table alist equal hash)
      (srfi-69:alist->hash-table alist equal (as-srfi-69-hash-function hash)))

    ;; Deviation 2. Keyed by identity, and hashed by it too: SRFI 69's
    ;; `hash-by-identity` has no branch for a record, so every table would land
    ;; in one bucket and each lookup would scan every immutable table ever
    ;; made. `equal-hash` separates them and is stable under mutation.
    (define immutable-tables
      (%make-hash-table eq? equal-hash))

    (define (immutable? ht)
      (hash-table-ref/default immutable-tables ht #f))

    (define (make-immutable! ht)
      (%hash-table-set! immutable-tables ht #t)
      ht)

    ;; Deviation 3.
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

    ;; SRFI 125 defines this as `(hash-table-set! ht key (updater
    ;; (hash-table-ref ht key failure success)))`, so it is written that way.
    (define (hash-table-update! ht key updater . failure+success)
      (%hash-table-set!
       ht key
       (updater (apply hash-table-ref ht key failure+success))))

    ;; Deviation 4.
    (define (hash-table-merge! ht1 ht2)
      (hash-table-walk
       ht2
       (lambda (key value)
         (if (not (hash-table-exists? ht1 key))
             (%hash-table-set! ht1 key value))))
      ht1))

  (include "125/hash.scm"))
