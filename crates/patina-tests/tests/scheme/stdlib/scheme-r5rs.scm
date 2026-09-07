;; `(scheme r5rs)` — the R7RS compatibility library, R7RS Appendix A.
;;
;; Migrated from `crates/patina-tests/tests/scheme_r5rs.rs` (#193 Phase 1).
;; 20 rows there; 19 are here and one stayed in Rust — see the note at the end.
;;
;; The library is a re-export surface, so what these rows mean to check is
;; *reachability*: that each name R5RS defines arrives through this one import,
;; whichever R7RS library it lives in now.
;;
;; **On Patina, only some of them can.** The top level always carries
;; `(scheme base)`'s exports whatever a program imports — deliberate, and pinned
;; by `import_set_is_enforced.rs::test_the_default_baseline_still_works`. So a
;; row naming a `(scheme base)` name would pass here with no import at all:
;; `dynamic-wind`, the numeric predicates, `string-copy`, the vector procedures
;; and `eof-object?` are all in that set. What those rows pin is that the name
;; *works*, not that this library supplies it.
;;
;; The rows that do test reachability are the ones naming something outside
;; `(scheme base)` — `(scheme char)`, `(scheme complex)`, `(scheme inexact)`,
;; `(scheme lazy)`, `(scheme cxr)`, `(scheme load)`, `(scheme eval)` — and the
;; two `exact->inexact`/`inexact->exact` aliases, which exist nowhere else. They
;; are marked below. On chibi and Gauche, where no such baseline exists, every
;; row tests reachability, which is one reason to keep running it there.
;;
;; **Two rows fail on an oracle, both theirs rather than ours.** chibi's
;; `(exact->inexact 1/3)` is not `equal?` to the literal `0.3333333333333333`
;; even though both print that way; Gauche's `(scheme r5rs)` `dynamic-wind`
;; returns the before-thunk instead of the body's value. Measured 2026-09-07.

(import (scheme r5rs) (srfi 64))

(test-begin "scheme-r5rs")

;; ── The exact/inexact spellings R5RS used ───────────────────────────────────

(test-equal "exact->inexact" 3.0 (exact->inexact 3))
(test-equal "inexact->exact" 3 (inexact->exact 3.0))
(test-equal "exact->inexact on a ratnum" 0.3333333333333333 (exact->inexact 1/3))

;; ── Names re-exported from the R7RS libraries that now hold them ────────────

(test-equal "char predicates, from (scheme char)" #t (char-alphabetic? #\a))
(test-equal "case-insensitive char comparison" #t (char-ci=? #\A #\a))
(test-equal "case-insensitive string comparison" #t (string-ci=? "ABC" "abc"))
(test-equal "complex constructors, from (scheme complex)" 3+4i (make-rectangular 3 4))
(test-equal "sqrt, from (scheme inexact)" 2.0 (sqrt 4.0))
(test-equal "trigonometry, from (scheme inexact)" 0.0 (sin 0.0))
(test-equal "delay and force, from (scheme lazy)" 3 (force (delay (+ 1 2))))
(test-equal "the deep c…r accessors, from (scheme cxr)" 42 (caaaar '((((42))))))
(test-equal "dynamic-wind (also in (scheme base), so reachability is not what this shows)" 99
  (dynamic-wind (lambda () #f) (lambda () 99) (lambda () #f)))
(test-equal "the numeric predicates (also in (scheme base))" '(#t #t #t #t #t)
  (list (positive? 1) (negative? -1) (odd? 3) (even? 4) (zero? 0)))
(test-equal "string procedures (also in (scheme base))" "hello" (string-copy "hello"))
(test-equal "vector procedures (also in (scheme base))" #(7 7 7)
  (let ((v (make-vector 3 0))) (vector-fill! v 7) v))
;; R5RS §6.6.2 has `eof-object?` but **no** `eof-object` — the procedure that
;; produces one is R7RS §6.13.2. The `.rs` row called `(eof-object)` and passed
;; here only through the baseline above; both oracles reject it, correctly. So
;; the portable row is the predicate on something that is not an eof object,
;; which is all R5RS gives a program to say.
(test-equal "eof-object?, which is the part R5RS defines" #f (eof-object? 'not-eof))
(test-equal "load is a procedure, from (scheme load)" #t (procedure? load))

;; ── eval and its environments ───────────────────────────────────────────────

;; R5RS's own two environment specifiers, both of which `(scheme r5rs)` must
;; carry along with `eval` itself.
(test-equal "eval in the report environment" 3
  (eval '(+ 1 2) (scheme-report-environment 5)))
(test-equal "eval in the interaction environment" 30
  (eval '(+ 10 20) (interaction-environment)))

;; ── Not here ────────────────────────────────────────────────────────────────
;;
;; One row could not come with the rest: that a *library body* importing only
;; `(scheme r5rs)` can still reach `define`, `lambda`, `if` and `quote`. Syntax
;; keywords are real bindings, so the library has to export them, and srfi-78's
;; reference implementation in the vendored corpus is exactly that shape — it
;; failed with `unbound variable: define` before they were exported.
;;
;; It cannot come here, and the reason is this file's own premise: the row needs
;; a `define-library` whose import set is *only* `(scheme r5rs)`, while this
;; file's top level has already imported `(srfi 64)` to have `test-equal` at
;; all. Nesting a library definition inside an SRFI 64 program would also stop
;; being the thing under test — the point is what the library body can see, not
;; what the program around it can.
;;
;; It went to `sld_file_loading.rs`, which already owns library-body and import
;; resolution, rather than keeping a whole test binary alive for one row.

(test-end)
