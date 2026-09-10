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
;;   Gauche                    1 pass, 3 omitted — see below
;;
;; **Gauche cannot compile three of the four rows**, as Patina could not until
;; 2026-08-25, by three separate refusals: `swap-first-two`'s definition is
;; "Pattern variable b is used in wrong level", the generated `first-of`'s use
;; is "malformed first-of", and `mention-dots`'s definition is "template's
;; ellipsis nesting is deeper than pattern's". chibi accepts all three.
;;
;; They sit in a `cond-expand` clause Gauche does not select, which is the only
;; thing that works: it refuses these programs while it **compiles** them, and
;; `test-skip` suppresses evaluation rather than compilation, so a skipped row
;; would still take the file down. An unselected clause is never compiled.
;;
;; **Why omit rather than let the file die and register it.** The earlier draft
;; did the latter, so that the lane held the claim and would report it if Gauche
;; ever accepted the program. That is the right instinct for a difference in an
;; *answer* — those stay unscoped and classified, which is how two Gauche bugs
;; came to be filed — but it is the wrong trade here, and the difference is
;; ownership. Whether Gauche compiles this is Gauche's conformance, not
;; Patina's behaviour; we have no fix to make and no drift to guard against. The
;; register earns its keep by stopping *our* rows being edited to match an
;; oracle, and there is nothing here to edit. Paying for that with Gauche's
;; arbitration of every other row in the file is a bad trade, and it gets worse
;; as the file grows.
;;
;; What the omission gives up is precisely nothing on our side: the driver's
;; floor for this file fails if a row stops running on Patina, which is the
;; direction that matters. What it gives up on the oracle side is the day
;; Gauche starts accepting these programs — nothing will notice, and this
;; header is the only record. That is a cost worth naming and worth paying.

(import (scheme base) (srfi 64))

(test-begin "ellipsis")

(define-syntax def-first
  (syntax-rules ()
    ((_ name) (define-syntax name (syntax-rules () ((_ a b (... ...)) (list a)))))))

(cond-expand
  (gauche)   ; cannot compile the three rows below — see the header
  (else
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
       (mention-dots 1)))))

;; A declared ellipsis (SRFI 46) is a *declaration*, so a binding of `...`
;; around it has no bearing on it either way. #114 looked up the spelling `...`
;; and broke this row while fixing the two above. chibi agrees; Gauche never
;; reaches it, for the reason in the header.
(test-equal "a declared ellipsis is unaffected by a binding of dots" '(2 3 1)
  (let ((... 'dots))
    (define-syntax m3 (syntax-rules ::: () ((_ a b :::) (list b ::: a))))
    (m3 1 2 3)))

(test-end)
