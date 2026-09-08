;; `(scheme eval)` — `eval` and `environment`, R7RS §6.12.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (Larceny
;; family 9, #193 Phase 1). The rest of `(scheme eval)`'s coverage is still in
;; `scheme_eval.rs`; this file exists so a known-open defect has somewhere to
;; live that retires itself.

(import (scheme base) (scheme eval) (srfi 64))

(test-begin "eval")

;; **A known-open defect, quarantined so it announces its own fix.**
;; `(prefix (only …) …)` is an ordinary import set, and `environment` should
;; accept whatever `import` accepts. Ours rejects it: "library name component
;; must be a symbol, got pair". chibi and Gauche both answer `1`, so this is
;; our gap and not a difference of opinion.
;;
;; The `.rs` row asserted only that this *errors*, with a comment telling the
;; next reader to replace the assertion by hand when it stops. SRFI 64 has the
;; mechanism: `test-expect-fail` plus the **right** answer, so the row is xfail
;; today and xpass the moment it is fixed — and the driver fails the build on
;; xpass, which is what makes a quarantine retire itself rather than wait to be
;; noticed.
;;
;; **The program is not the `.rs` one, because that one could never pass.** It
;; used `(eval '(p:car '(1 2)) …)`, and the environment it builds holds exactly
;; one binding — `p:car`. `quote` is not in it, so `'(1 2)` cannot be
;; evaluated: chibi and Gauche both get past `environment` and then fail with
;; "invalid application: (1 2)", because `(quote (1 2))` is an application of an
;; unbound `quote`. The documented fixed answer of `1` was unreachable. Built
;; from `cons` instead, the row means what it says, and the oracles reach the
;; answer the quarantine is waiting for.
;;
;; Scoped, because the expectation is about our gap alone.
(cond-expand (patina (test-expect-fail 1)) (else))
(test-equal "environment accepts a nested import set" 1
  (eval '(p:car (p:cons 1 2))
        (environment '(prefix (only (scheme base) car cons) p:))))

(test-end)
