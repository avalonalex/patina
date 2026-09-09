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
;; **Both binding rows put both macros in one `let`.** They assert one each so
;; that a failure names itself, but neither could be written with only the
;; macro it asks about: the shape that defeats a per-macro or per-scope rule is
;; a single scope in which one macro has an ellipsis and the other does not.
;; Split into a scope apiece, an implementation deciding per scope by looking
;; at whichever macro it finds there would pass both.
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   4 pass
;;   chibi                     4 pass
;;   Gauche                    does not complete — registered
;;
;; **Gauche rejects three of the four rows**, as Patina used to, by three
;; separate refusals: `swap-first-two`'s definition is "Pattern variable b is
;; used in wrong level", the generated `first-of`'s use is "malformed
;; first-of", and `mention-dots`'s definition is "template's ellipsis nesting
;; is deeper than pattern's". chibi accepts all three. (The Rust row this came from also named Larceny, Kawa
;; and Sagittarius as accepting it; that claim is inherited from its comment,
;; is in no document this repo holds, and was not re-measured here.)
;;
;; Gauche refuses these programs while it **compiles** them, which is why the
;; file is registered `*` — `incomplete` — rather than scoped. `test-skip`
;; suppresses evaluation, not compilation, so it cannot save a row Gauche will
;; not read; and a `cond-expand` clause that omits the rows would hide a real
;; difference from the lane, since an absent row cannot FAIL. Letting the file
;; die puts the difference in `DIVERGENCES.tsv`, where the lane checks it and
;; where it retires itself the day Gauche accepts the program. The price is
;; the one row Gauche would pass — the declared-ellipsis row; chibi still
;; arbitrates it, and every other row here.

(import (scheme base) (srfi 64))

(test-begin "ellipsis")

(define-syntax def-first
  (syntax-rules ()
    ((_ name) (define-syntax name (syntax-rules () ((_ a b (... ...)) (list a)))))))

;; `swap-first-two` is written inside the binding, so its `...` is the variable
;; `dots` and the pattern binds three variables: `(list b a ...)` puts them back
;; in the order `b a dots`, where a real ellipsis would splice.
(test-equal "a syntax-rules written where dots is bound has no ellipsis"
  '(2 1 3)
  (let ((... 'dots))
    (define-syntax swap-first-two
      (syntax-rules () ((_ a b ...) (list b a ...))))
    (def-first first-of)
    (swap-first-two 1 2 3)))

;; The opposite direction in the same scope, and the half a per-macro decision
;; loses: `def-first` is written at top level and escapes an ellipsis into what
;; it generates. That token is an ellipsis at the use site whatever `...` means
;; there, so `first-of` takes one argument and any number more.
(test-equal "an escaped ellipsis is still one inside a binding of dots"
  '(1)
  (let ((... 'dots))
    (define-syntax swap-first-two
      (syntax-rules () ((_ a b ...) (list b a ...))))
    (def-first first-of)
    (first-of 1 2 3 4)))

;; A template may *refer* to a definition-site local spelled `...`, which is
;; the same claim `expansion/hygiene.scm` makes for `if` in its first row —
;; here rather than there because Gauche rejects this one too, with a third
;; distinct message ("template's ellipsis nesting is deeper than pattern's"),
;; and one file already carries the cost of that. Reporting the reference as a
;; keyword rather than a variable was one of the pair gating Larceny's `base`
;; at load; fixed 2026-08-25.
;;
;; `if` is bound alongside `...` though nothing here refers to it, for the
;; reason the sibling row in `hygiene.scm` binds `...`: the Rust original had
;; one `let` binding both and a macro per spelling, so one body had to serve
;; two keyword-spelled locals. Each half keeps that body.
(test-equal "a template may refer to a definition-site local spelled ..."
  '(1 dots)
  (let ((... 'dots) (if 'nineteen))
    (define-syntax mention-dots (syntax-rules () ((_ a) (list a ...))))
    (mention-dots 1)))

;; A declared ellipsis (SRFI 46) is a *declaration*, so a binding of `...`
;; around it has no bearing on it either way. #114 looked up the spelling `...`
;; and broke this row while fixing the two above. chibi agrees; Gauche never
;; reaches it, for the reason in the header.
(test-equal "a declared ellipsis is unaffected by a binding of dots" '(2 3 1)
  (let ((... 'dots))
    (define-syntax m3 (syntax-rules ::: () ((_ a b :::) (list b ::: a))))
    (m3 1 2 3)))

(test-end)
