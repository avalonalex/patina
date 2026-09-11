;; SRFI 132 sorting and selection.
;;
;; Migrated from `crates/patina-tests/tests/srfi_132_list_sort_stability.rs`
;; and `srfi_132_select.rs` (#193), both deleted. Upstream's own suite runs in
;; `upstream_srfi_suites.rs`; these rows pin what it does not reach.

(import (scheme base) (srfi 132) (srfi 64))

(test-begin "sorting")

;; `list-sort` is stable — a deliberate, marked deviation from the bundled
;; reference implementation (why: the PATINA LOCAL EDIT in
;; `lib/srfi/132/sort.scm` and `132.sld`'s header), pinned so a revert to
;; upstream's tie-reversing heap sort cannot land silently. SRFI 132 does not
;; require `list-sort` to be stable, so an implementation that reversed the
;; ties would not be wrong; this row asserts Patina's choice.
(test-equal "list-sort keeps equal elements in their original order"
  '((b . 2) (d . 2) (a . 1) (c . 1) (e . 1))
  (list-sort (lambda (x y) (> (cdr x) (cdr y)))
             '((a . 1) (b . 2) (c . 1) (d . 2) (e . 1))))

;; `vector-select!` and `vector-find-median` only enter their real quickselect
;; path once the range reaches 50 elements (`just-sort-it-threshold` in
;; `lib/srfi/132/select.scm`); below that they punt to a full sort. The bundled
;; reference suite never exceeds about 12 elements, so nothing exercised the
;; pivot-choosing RNG — which shipped calling unbound identifiers and crashed
;; every large range (audit item A1, `PRD/ARCHIVE/AUDIT_2026_08_10_PRD.md`).

;; A 100-element reversed vector: the k-th smallest is simply k.
(test-equal "vector-select! past the sort threshold" 50
  (let ((v (make-vector 100)))
    (do ((i 0 (+ i 1))) ((= i 100)) (vector-set! v i (- 99 i)))
    (vector-select! < v 50)))

(test-equal "vector-find-median past the sort threshold" 30
  (let ((v (make-vector 61)))
    (do ((i 0 (+ i 1))) ((= i 61)) (vector-set! v i (- 60 i)))
    (vector-find-median < v 0)))

;; Duplicates drive the equal-to-pivot partition paths: 30 each of 0, 1 and 2.
(define (thirty-each)
  (let ((v (make-vector 90)))
    (do ((i 0 (+ i 1))) ((= i 90)) (vector-set! v i (modulo i 3)))
    v))

(test-equal "vector-select! with duplicates" '(0 1 2)
  (list (vector-select! < (thirty-each) 10)
        (vector-select! < (thirty-each) 45)
        (vector-select! < (thirty-each) 89)))

;; SRFI 132: "Elements within the range may be reordered, whereas those
;; outside the range are left alone." Reordered, not replaced. The Rust
;; original relied on this without saying so — it selected three times from
;; one vector, correct only if the first selection left the same elements
;; behind — and that shape answered differently on Gauche from run to run,
;; because what its selection destroys depends on random pivots. This row
;; asks the question directly, and answers the same way every time.
(test-assert "vector-select! only reorders the vector"
  (let ((v (thirty-each)))
    (vector-select! < v 45)
    (equal? (list-sort < (vector->list v))
            (list-sort < (vector->list (thirty-each))))))

(test-end)
