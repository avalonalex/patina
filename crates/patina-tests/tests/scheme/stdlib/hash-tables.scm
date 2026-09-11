;; SRFI 125 hash tables, and the SRFI 69 layer under them — what chibi's own
;; SRFI 125 suite cannot see.
;;
;; Migrated from `crates/patina-tests/tests/srfi_125_hash_tables.rs` (#193
;; Phase 2), which is deleted. The upstream suite is the headline gate — 74
;; assertions, run on both backends by `upstream_srfi_suites.rs` — and covers
;; the library far better than a hand-written file would. So this one holds
;; only the cases it *misses*, each of which was a real defect found by review
;; after the suite was green:
;;
;;   - a hash function handed straight to the constructor, which only works
;;     because SRFI 125 adapts every hash function on the way in;
;;   - `hash-table-update!`'s `success` argument, which the suite calls with
;;     at most four arguments;
;;   - `(srfi 69)`'s own `hash` on inexact and non-rational keys, and an `eq?`
;;     table hashing by identity, which the suite never calls.
;;
;; `(scheme hash-table)`'s export list is checked against `(srfi 125)`'s by
;; `r7rs_large_aliases.rs`, in both directions — a stronger check than
;; sampling bindings here.
;;
;; SRFI 69 is imported under a prefix. It shares most of its names with
;; SRFI 125, and whether an implementation's two libraries export the *same*
;; bindings for them is its own business; the prefix keeps the file from
;; depending on it.

(import (scheme base) (srfi 125) (srfi 128) (prefix (srfi 69) s69:) (srfi 64))

(test-begin "hash-tables")

;; ── SRFI 125 ───────────────────────────────────────────────────────────────

;; A hash function handed straight to the constructor — which the upstream
;; suite does at its `ht-symbol` fixture, extracting
;; `(comparator-hash-function default-comparator)` and passing it along. It
;; is a *one*-argument procedure, while our SRFI 69 always supplies the bound,
;; so it only works because SRFI 125 adapts every hash function on the way in
;; rather than only the ones it reads out of a comparator. Adapting only the
;; comparator path passes every other row here and fails the upstream suite.
(test-equal "a hash function passed by hand is adapted" '(1 2 2)
  (let ((ht (alist->hash-table '((a . 1) (b . 2))
                               equal?
                               (comparator-hash-function (make-default-comparator)))))
    (list (hash-table-ref/default ht 'a 'missing)
          (hash-table-ref/default ht 'b 'missing)
          (hash-table-size ht))))

;; SRFI 125 defines `hash-table-update!` as
;; `(hash-table-set! ht key (updater (hash-table-ref ht key failure success)))`
;; — so `success` transforms the value *before* the updater sees it. SRFI 69's
;; version takes only a failure thunk and has a rest argument, so an inherited
;; one accepted `success` and silently ignored it: the update below produced 8
;; instead of 71.
(test-equal "hash-table-update! applies success before the updater" '(71 10)
  (let ((ht (make-hash-table (make-equal-comparator))))
    (hash-table-set! ht 'k 7)
    (hash-table-update! ht 'k (lambda (v) (+ v 1)) (lambda () 0) (lambda (v) (* v 10)))
    (hash-table-update! ht 'absent (lambda (v) (* v 2)) (lambda () 5))
    (list (hash-table-ref/default ht 'k #f)
          (hash-table-ref/default ht 'absent #f))))

;; ── SRFI 69, under SRFI 125 ────────────────────────────────────────────────

;; `hash` feeds a `vector-ref` index, so an inexact result crashes the table
;; outright — which it did, for every inexact key. Two branches produce one:
;; `real?` reaches `numerator`/`denominator`, which R7RS makes inexact for
;; inexact input, and `integer?` matches 2.0 first. The first fix covered only
;; `real?`, so 2.0 still crashed — hence a case from each branch, plus a
;; composite that recurses back into `hash`.
(test-equal "inexact keys hash to exact values"
  '(#t #t #t #t real-branch integer-branch)
  (let ((ht (s69:make-hash-table)))
    (s69:hash-table-set! ht 2.718 'real-branch)
    (s69:hash-table-set! ht 2.0 'integer-branch)
    (list (exact-integer? (s69:hash 2.718))
          (exact-integer? (s69:hash 2.0))
          (exact-integer? (s69:hash 1e10))
          (exact-integer? (s69:hash (vector 'a 2.0)))
          (s69:hash-table-ref/default ht 2.718 'missing)
          (s69:hash-table-ref/default ht 2.0 'missing))))

;; D7 — `hash` reached `numerator` for any real that is not rational, so the
;; infinities and NaN raised instead of hashing. The `exact` wrap that fixed
;; 2.0 and the rationals did not reach them.
(test-equal "non-rational reals are hashable" '(#t #t #t inf)
  (let ((ht (s69:make-hash-table)))
    (s69:hash-table-set! ht +inf.0 'inf)
    (list (exact-integer? (s69:hash +inf.0))
          (exact-integer? (s69:hash -inf.0))
          (exact-integer? (s69:hash +nan.0))
          (s69:hash-table-ref/default ht +inf.0 'missing))))

;; `(make-hash-table eq?)` keys by identity, so it must *hash* by identity
;; too. Upstream's `hash-by-identity` is the structural `hash`, and SRFI 125's
;; `make-eq-comparator` routes here, so the mismatch was load-bearing. Three
;; failures, one per way structure and identity part company: a key mutated
;; after insertion moves out of its bucket, a procedure has no structure to
;; hash, and a circular one has no end.
(test-equal "an eq? table hashes by identity" '(v v v)
  (list (let ((ht (s69:make-hash-table eq?))
              (k (vector 1 2 3)))
          (s69:hash-table-set! ht k 'v)
          (vector-set! k 0 99)
          (s69:hash-table-ref/default ht k 'missing))
        (let ((ht (s69:make-hash-table eq?)))
          (s69:hash-table-set! ht car 'v)
          (s69:hash-table-ref/default ht car 'missing))
        (let ((ht (s69:make-hash-table eq?))
              (circ (list 1 2)))
          (set-cdr! (cdr circ) circ)
          (s69:hash-table-set! ht circ 'v)
          (s69:hash-table-ref/default ht circ 'missing))))

;; Identity hashing must still agree with `eq?` on everything `eq?` accepts,
;; and must still tell distinct objects apart.
(define-record-type <p> (mk a) p? (a p-a))
(test-equal "identity hashing agrees with eq?"
  '(found found found found found found found (A B))
  (let ((round-trip (lambda (key)
                      (let ((h (s69:make-hash-table eq?)))
                        (s69:hash-table-set! h key 'found)
                        (s69:hash-table-ref/default h key 'missing))))
        (s (string #\s)))
    (list (round-trip (mk 1)) (round-trip 'sym) (round-trip 42) (round-trip #\a)
          (round-trip #t) (round-trip '()) (round-trip s)
          (let ((h (s69:make-hash-table eq?)) (a (vector 1)) (b (vector 1)))
            (s69:hash-table-set! h a 'A)
            (s69:hash-table-set! h b 'B)
            (list (s69:hash-table-ref/default h a '?)
                  (s69:hash-table-ref/default h b '?))))))

(test-end)
