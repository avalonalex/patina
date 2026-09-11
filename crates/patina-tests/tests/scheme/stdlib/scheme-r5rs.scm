;; `(scheme r5rs)` — the R7RS compatibility library, R7RS Appendix A.
;;
;; Migrated from `crates/patina-tests/tests/scheme_r5rs.rs` (#193 Phase 1).
;; 20 rows there; 19 are here, one stayed in Rust (see the note at the end), and
;; one was added — `(scheme file)`, which the library re-exports and nothing
;; tested. So 20 rows.
;;
;; Both oracles run it with one failure apiece, each their own and both recorded
;; below: chibi 19/20, Gauche 19/20, both Patina backends 20/20.
;;
;; The library is a re-export surface, so what these rows mean to check is
;; *reachability*: that each name R5RS defines arrives through this one import,
;; whichever R7RS library it lives in now.
;;
;; **On Patina, only some of them can.** The top level carries `(scheme base)`'s
;; exports whatever a program imports — measured, not merely assumed:
;; `(import (scheme cxr)) (car (list 1 2))` answers 1.
;; `import_set_is_enforced.rs::test_the_default_baseline_still_works` pins only
;; the *no-import* half of that, so if import sets were ever tightened to narrow
;; the top level, that test would still pass while the rows below quietly
;; changed meaning. In the cargo lane there is a second masker besides: the
;; driver evaluates `(import (scheme base) (srfi 64))` into the same interpreter
;; before the file runs (`scheme_suite.rs`), so those rows would pass there even
;; with the baseline gone. So a row naming a `(scheme base)` name shows nothing
;; about this library:
;; `dynamic-wind`, the numeric predicates, `string-copy`, the vector procedures
;; and `eof-object?` are all in that set. What those rows pin is that the name
;; *works*, not that this library supplies it.
;;
;; The rows that do test reachability are the ones naming something outside
;; `(scheme base)` — `(scheme char)`, `(scheme complex)`, `(scheme inexact)`,
;; `(scheme lazy)`, `(scheme cxr)`, `(scheme load)`, `(scheme eval)`,
;; `(scheme file)` — and the two `exact->inexact`/`inexact->exact` aliases,
;; which no *R7RS* library provides (`lib/rnrs/r5rs.sld`, `lib/r6rs/r5rs.sld`
;; and `lib/srfi/113.sld` reach them, the last by importing this library). On chibi and Gauche, where no such baseline exists, every
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
;; Compared against `(/ 1.0 3.0)` rather than the literal 0.3333333333333333,
;; because the literal made the row a test of the *reader* too. Measured
;; 2026-09-11: chibi 0.12's reader on arm64 (Homebrew) reads that literal one
;; ulp low — `(exact 0.3333333333333333)` is 6004799503160660/2^54 there,
;; where Patina, Gauche and chibi on x86-64 give the correctly rounded
;; 6004799503160661/2^54 — while its `exact->inexact` is right. The row was
;; registered against chibi as needs-investigation until the oracle lane ran
;; on an x86-64 CI runner and stopped reproducing it.
(test-equal "exact->inexact on a ratnum" (/ 1.0 3.0) (exact->inexact 1/3))

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
;;
;; That is weaker than the row it replaces — a stub `(lambda (x) #f)` satisfies
;; it — and R5RS offers no portable way to *make* an eof object to strengthen
;; it. The positive case lives where a program can actually reach one:
;; `read_consumption.rs`, `vfs_file_io.rs` and `stdlib/ports.scm`.
(test-equal "eof-object?, which is the part R5RS defines" #f (eof-object? 'not-eof))
(test-equal "load is a procedure, from (scheme load)" #t (procedure? load))

;; `(scheme file)` was re-exported and untested. Nothing masks it — with no
;; import, `open-input-file` is an unbound variable — so this is one of the rows
;; that does test reachability, and it would have gone unnoticed if the library
;; stopped re-exporting it.
(test-equal "file procedures, from (scheme file)" '(#t #t #t)
  (list (procedure? open-input-file)
        (procedure? call-with-output-file)
        (procedure? with-input-from-file)))

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
;; `(scheme r5rs)` still reaches `define`, `lambda`, `if` and `quote`. It is
;; `sld_file_loading.rs::test_a_library_body_importing_only_scheme_r5rs_has_core_syntax`,
;; which carries the reason — chibi cannot run it, so it cannot live in a file
;; this suite runs on chibi. Recorded there rather than in both places.

(test-end)
