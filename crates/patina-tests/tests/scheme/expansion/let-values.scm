;; `let-values` and `let*-values` — R7RS §4.2.2.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (#193
;; Phase 1), from its "What `base` found once it ran" section. Its sibling
;; `expansion/define-values.scm` covers §5.3.3's *definition* form; this is the
;; expression form, and the distinction is the whole content of the row below.
;;
;; `let-values` evaluates every init in the **outer** environment before
;; binding any formal — that is what makes it parallel. Ours bound each clause
;; before evaluating the next, which is `let*-values`, until 2026-08-25 (it is
;; now R7RS §7.3's reference implementation). The two forms are asserted side
;; by side because the bug made them identical, and only comparing them shows
;; it: `(x y a b)` against `(x y x y)`.
;;
;; Also covered: a dotted formal and a rest formal, which the grammar allows
;; and which a naive expansion drops. Not covered: a `(() (values))` clause,
;; which works on the VM and not the tree-walker — that is Larceny family 18,
;; zero values arriving as one, and it belongs with the divergences.

(import (scheme base) (srfi 64))

(test-begin "let-values")

(test-equal "let-values binds in parallel, let*-values in sequence"
  '((x y a b) (1 (2 3) (4 5)) (x y x y))
  (let ((a 'a) (b 'b) (x 'x) (y 'y))
    (list (let-values (((a b) (values x y)) ((x y) (values a b)))
            (list a b x y))
          (let-values (((p . q) (values 1 2 3)) (r (values 4 5)))
            (list p q r))
          (let*-values (((a b) (values x y)) ((x y) (values a b)))
            (list a b x y)))))

(test-end)
