;; Quasiquote denotes structure, not the procedures that would build it —
;; R7RS §4.2.8.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (Larceny
;; family 34, #193 Phase 1). Its banner read "VM: quasiquote built its result
;; with the use site's `list`", which names where the defect was; the row is a
;; plain both-backend assertion and, measured 2026-09-09, chibi and Gauche
;; answer it identically.
;;
;; A quasiquote means the structure it writes whatever `list`, `append` and
;; `list->vector` happen to mean where it appears. The VM's expansion called
;; them **by name**, so under SRFI 101 — whose `list` builds random-access
;; lists — `` `(1 ,x 3) `` was one too, and `` `#(1 ,x) `` failed inside
;; `list->vector`. The tree-walker built the structure directly and was right
;; all along. Fixed 2026-08-26: the expansion references the registry's
;; primitives as values, so nothing a program imports or defines can redirect
;; them.
;;
;; The import set is the test. `(scheme base)`'s list operations are excluded
;; and re-imported under `r7:`, and SRFI 101 supplies the bare names — so the
;; last element, `(r7:pair? (list 1 2))` answering `#f`, is what proves the
;; rebinding actually took effect. Without it the row would pass on an
;; implementation where the import did nothing.

(import (scheme write)
        (except (scheme base) quote car cons list list? append)
        (prefix (scheme base) r7:)
        (srfi 101)
        (srfi 64))

(test-begin "quasiquote")

(define x 2)

;; The expected value is built with `r7:list`, not written as `'(#t #t #t #f)`.
;; `quote` is excluded from `(scheme base)` above and **SRFI 101 exports its
;; own**, which builds a random-access list — so the literal would not be a
;; pair list and the row would fail everywhere, as the first draft did on all
;; three implementations. The import set that makes this row a test also makes
;; ordinary Scheme notation unavailable inside it.
(test-equal "quasiquote builds pairs whatever list means at the use site"
  (r7:list #t #t #t #f)
  (r7:list (r7:pair? `(1 ,x 3))
           (r7:equal? `(1 ,@(r7:list 7 8) 3) (r7:list 1 7 8 3))
           (r7:vector? `#(1 ,x))
           (r7:pair? (list 1 2))))   ; SRFI 101's list, so not a pair

(test-end)
