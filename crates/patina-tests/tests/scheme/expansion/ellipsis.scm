;; The ellipsis is identified by *binding*, not by spelling — R7RS §4.3.2, and
;; SRFI 46 for the declared form.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (families 14
;; and 15, #193 Phase 1), where the rows sat under the suite run that found
;; them. Their subject is one question asked three ways: *which token in this
;; template is the ellipsis?* — and the answer is never "whichever one is
;; spelled `...`".
;;
;; §4.3.2 says a `syntax-rules` written where `...` is bound as a variable has
;; no ellipsis at all, so `(_ a b ...)` is a three-variable pattern. Patina
;; rejected such a definition until 2026-08-25, and deciding the question once
;; per macro (#111) then got the *written* macro right and lost the *generated*
;; one — a macro defined outside the binding escapes an ellipsis into its
;; output with `(... ...)`, and that one is an ellipsis even though the macro
;; it lands in is compiled inside the binding. The rule asks per token now: a
;; token carrying an identity of its own — which an escaped `(... ...)` does —
;; is read in its own scopes, and every other in the macro's definition scopes.
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   3 pass
;;   chibi                     3 pass
;;   Gauche                    1 pass, 2 omitted — see below
;;
;; **Gauche rejects both binding rows**, as Patina used to, and by two separate
;; refusals: `swap-first-two`'s definition is "Pattern variable b is used in
;; wrong level", and the generated `first-of`'s *use* is "malformed first-of".
;; chibi accepts both, as do Larceny, Kawa and Sagittarius per the family-14
;; triage, so this is Gauche alone.
;;
;; They are omitted on Gauche with `cond-expand` rather than skipped with
;; `test-skip`, and that is not a style preference — it is the one case where
;; the file's usual scoping tool cannot work. Gauche refuses these programs
;; while it **compiles** the enclosing form; `test-skip` suppresses evaluation
;; only, so a skipped row would still take the whole file down and cost Gauche
;; the third row too. A `cond-expand` clause that is not selected is never
;; compiled. The cost is that the omission is invisible in the lane's output —
;; an absent row cannot FAIL — which is why it is spelled out here.

(import (scheme base) (srfi 64))

(test-begin "ellipsis")

(define-syntax def-first
  (syntax-rules ()
    ((_ name) (define-syntax name (syntax-rules () ((_ a b (... ...)) (list a)))))))

(cond-expand
  (gauche)   ; rejects both rows at compile time — see the header
  (else
   ;; Written inside the binding, so its `...` is the variable `dots` and the
   ;; pattern binds three variables: `(list b a ...)` puts them back in the
   ;; order `b a dots`, where a real ellipsis would splice.
   (test-equal "a syntax-rules written where dots is bound has no ellipsis"
     '(2 1 3)
     (let ((... 'dots))
       (define-syntax swap-first-two
         (syntax-rules () ((_ a b ...) (list b a ...))))
       (swap-first-two 1 2 3)))

   ;; The opposite direction, and the half that a per-macro decision loses:
   ;; `def-first` is written at top level and escapes an ellipsis into what it
   ;; generates. That token is an ellipsis at the use site whatever `...` means
   ;; there, so `first-of` takes one argument and any number more.
   (test-equal "an escaped ellipsis is still one inside a binding of dots"
     '(1)
     (let ((... 'dots))
       (def-first first-of)
       (first-of 1 2 3 4)))))

;; A declared ellipsis (SRFI 46) is a *declaration*, so a binding of `...`
;; around it has no bearing on it either way. #114 looked up the spelling `...`
;; and broke this row while fixing the two above. All three implementations
;; agree here.
(test-equal "a declared ellipsis is unaffected by a binding of dots" '(2 3 1)
  (let ((... 'dots))
    (define-syntax m3 (syntax-rules ::: () ((_ a b :::) (list b ::: a))))
    (m3 1 2 3)))

(test-end)
