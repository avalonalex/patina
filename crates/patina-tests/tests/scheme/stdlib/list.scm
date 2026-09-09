;; `(scheme list)` — SRFI 1's n-ary procedures, the ones that walk more than
;; one list at a time.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (Larceny
;; family 5, #193 Phase 1). Its banner read "tree-walker: SRFI 1's n-ary
;; procedures raise a wrong-arity error", which names where the defect was, not
;; what the row asserts — measured 2026-09-09, chibi and Gauche both answer
;; exactly this.
;;
;; The interesting part is that the cause was not in SRFI 1 or in `apply`.
;; `zip` is `(apply map list list1 more-lists)` inside
;; `srfi-1-reference.scm`, and SRFI 1's `%cars+cdrs` bails out of an exhausted
;; list with `(abort '() '())` — invoking a continuation with **two** values,
;; which the tree-walker refused. That is Larceny family 17, now
;; `control/values.scm`'s third row, and fixing it fixed every n-ary procedure
;; here at once. Six of them are listed because "every procedure that walks
;; more than one list" is the claim, and one would not show it.

(import (scheme base) (scheme list) (srfi 64))

(test-begin "list")

;; One row each rather than the `.rs` file's single six-element list. They
;; share a cause, which the comment above records — but they are six separately
;; implemented procedures, and a later regression need not be in the shared
;; cause. Packed together, a break in `filter-map` alone prints a six-element
;; list for the reader to scan; apart, the failure names itself.
(test-equal "zip walks two lists" '((1 4) (2 5) (3 6))
  (zip '(1 2 3) '(4 5 6)))

(test-equal "fold walks two lists" 10
  (fold + 0 '(1 2) '(3 4)))

(test-equal "every walks two lists" #t
  (every < '(1 2) '(3 4)))

(test-equal "any walks two lists" 'yes
  (any (lambda (a b) (if (< a b) 'yes #f)) '(1 2 3) '(0 1 4)))

(test-equal "filter-map walks two lists" '(9 27)
  (filter-map (lambda (x y) (and (number? x) (* x y))) '(a 1 b 3) '(9 9 9 9)))

(test-equal "list-index walks two lists" 1
  (list-index = '(1 2 3) '(9 2 9)))

(test-end)
