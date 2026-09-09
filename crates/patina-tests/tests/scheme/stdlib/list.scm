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

(test-equal "SRFI 1's n-ary procedures walk more than one list"
  '(((1 4) (2 5) (3 6)) 10 #t yes (9 27) 1)
  (list (zip '(1 2 3) '(4 5 6))
        (fold + 0 '(1 2) '(3 4))
        (every < '(1 2) '(3 4))
        (any (lambda (a b) (if (< a b) 'yes #f)) '(1 2 3) '(0 1 4))
        (filter-map (lambda (x y) (and (number? x) (* x y))) '(a 1 b 3) '(9 9 9 9))
        (list-index = '(1 2 3) '(9 2 9))))

(test-end)
