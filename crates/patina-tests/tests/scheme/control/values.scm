;; Multiple values — R7RS §6.10: `values`, `call-with-values`, and a
;; continuation invoked with other than one value.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (Larceny
;; family 4, #193 Phase 1). Its banner there read "VM: a discarded call to
;; `values` poisons the next", which says where the defect was rather than what
;; the rows assert: every one is a plain both-backend assertion, and measured
;; 2026-09-09 chibi and Gauche agree with all of them. They were never
;; backend-specific.
;;
;; The rule underneath all four: **one value is itself, any other count is a
;; `#<values>` object**. The VM used to keep several values in a side buffer
;; instead, which a discarded `values` call left set; the tree-walker wrote the
;; rule out in four places and one copy special-cased zero. Both were fixed on
;; 2026-08-25, and one `Heap::values_from` now serves every path.
;;
;; The row below it, "a continuation can be handed multiple values", came from
;; `cps-features.scm` in the same change. It was nearly left there because it
;; carries a `DIVERGENCES.tsv` entry and moving it means editing the register —
;; which is placement by bookkeeping rather than by subject, and the same
;; mistake this whole redistribution exists to undo. The register edit is one
;; line, and `every_registered_divergence_names_a_real_row` catches a stale one.

(import (scheme base) (srfi 64))

(test-begin "values")

;; The original leak: `values` called with one argument in a non-tail position
;; and thrown away. The next `call-with-values`, whose producer returns a plain
;; value, must not see it.
(define (call1 f) (f 42))
(test-equal "a discarded values call does not leak into the next" '(fresh)
  (begin
    (call1 values)
    (call-with-values (lambda () 'fresh) (lambda xs xs))))

;; The shapes the removed buffer used to carry, now through the value itself: a
;; producer that calls `values` for effect and then returns something else,
;; several values, and a primitive that returns several.
(define (call3 f) (f 1 2 3))
(test-equal "call-with-values sees only what its producer returned"
  '((one) (1 2) (4 1))
  (list (call-with-values (lambda () (call3 values) 'one) (lambda xs xs))
        (call-with-values (lambda () (values 1 2)) list)
        (call-with-values (lambda () (exact-integer-sqrt 17)) list)))

;; A continuation invoked with other than one value delivers a `#<values>`
;; object, exactly as `(values …)` returns one — the VM since #113, the
;; tree-walker since 2026-08-25. Refusing this is what made SRFI 1's n-ary
;; procedures unusable there; see `stdlib/list.scm`.
(test-equal "a continuation invoked with two values delivers them" '(4 5)
  (call-with-values (lambda () (call/cc (lambda (k) (k 4 5)))) list))

;; The same claim reached the other way: a `values` object handed to a
;; continuation, rather than two arguments. Gauche delivers only the first —
;; see the register. The `.rs` row this came from compared "1\n2\n3", which was
;; the harness rendering a values object rather than anything a program can
;; see; `call-with-values` is how a program asks.
(test-equal "a continuation can be handed multiple values" '(1 2 3)
  (call-with-values
    (lambda () (call-with-current-continuation (lambda (k) (k (values 1 2 3)))))
    list))

;; Zero is the count the rule's four copies disagreed about: the tree-walker
;; gave `(#<unspecified>)` for the first of these until 2026-08-25.
(test-equal "zero values reach the consumer as no arguments" '(() ())
  (list (call-with-values (lambda () (values)) (lambda xs xs))
        (call-with-values (lambda () (call/cc (lambda (k) (k)))) (lambda xs xs))))

;; ── The report's examples ──────────────────────────────────────────────────
;;
;; From `crates/patina-tests/tests/compliance/control.rs` (#193): the ordinary
;; shapes of R7RS §6.10, most of them the report's own examples.

(test-equal "values of one argument is that argument" 42 (values 42))
(test-equal "call-with-values, the report's example" 5
  (call-with-values (lambda () (values 4 5)) (lambda (a b) b)))
(test-equal "three values to a three-argument consumer" 6
  (call-with-values (lambda () (values 1 2 3)) (lambda (a b c) (+ a b c))))
;; `*` with no arguments is 1, and `-` of one argument negates it.
(test-equal "call-with-values of two primitives, the report's example" -1
  (call-with-values * -))
(test-equal "two values summed by the consumer" 3
  (call-with-values (lambda () (values 1 2)) (lambda (a b) (+ a b))))

(test-end)
