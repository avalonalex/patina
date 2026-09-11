;; The ellipsis is identified by *binding*, not by spelling — R7RS §4.3.2, and
;; SRFI 46 for the declared form.
;;
;; **Assembled from two migrations**, both under #193: families 14 and 15 of
;; `larceny_families.rs`, where the rows sat under the suite run that found
;; them, and `hygiene.rs`'s ellipsis-escape tests (five of them, three rows —
;; the section below says why three).
;;
;; Two subjects, and §4.3.2 is the source of both: *which token in this template
;; is the ellipsis?* — never "whichever one is spelled `...`" — and what the
;; escape `(... ⟨template⟩)` hands back.
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
;;   patina VM / tree-walker   7 pass
;;   chibi                     7 pass
;;   Gauche                    4 pass, 3 omitted — see below
;;
;; Re-measured 2026-09-11 with five rows from `compliance/macros_advanced.rs`
;; (#193) — SRFI 46's declared ellipsis and `...` as a literal, and the escape
;; in a generated macro: 12 pass on both backends and chibi, 9 pass and the
;; same 3 omitted on Gauche, which compiles all five new rows.
;;
;; **Gauche cannot compile three of the seven rows**, as Patina could not until
;; 2026-08-25, by three separate refusals: `swap-first-two`'s definition is
;; "Pattern variable b is used in wrong level", the generated `first-of`'s use
;; is "malformed first-of", and `mention-dots`'s definition is "template's
;; ellipsis nesting is deeper than pattern's". chibi accepts all three. (The
;; Rust row these came from also named Larceny, Kawa and Sagittarius as
;; accepting them; that claim is inherited from its comment, is in no document
;; this repo holds, and has not been re-measured — but it is why Gauche reads
;; as the outlier here rather than as one of two camps.) **Not reported
;; upstream**: nobody has checked Gauche's tracker for it. The two Gauche bugs
;; filed from this suite, shirok/Gauche#1326 and #1327, are different rows.
;;
;; They sit in a `cond-expand` clause Gauche does not select. `test-skip`
;; cannot do this job — Gauche refuses these programs while it **compiles**
;; them, and a skipped row is still compiled, so it would take the file down
;; anyway. An unselected `cond-expand` clause is never compiled at all.
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


;; ── What the escape produces ────────────────────────────────────────────────
;;
;; **From `hygiene.rs`** (#193). R7RS §4.3.2 gives `(... ⟨template⟩)` as an
;; escape: the template is used with `...` treated as an ordinary symbol. The
;; binding rows elsewhere in this file ask *which token is the ellipsis*; these
;; ask what the escape hands back, which is the other half of the same
;; sentence.
;;
;; Five Rust tests became three rows. Two of the five asserted the same claims
;; through `equal?` — `(equal? (make-ellipsis) '...)` beside a printed-form
;; check of the same call — and `test-equal` compares with `equal?`, so those
;; two are what the three rows already say. The printed form is not lost so
;; much as not the point: the claim is that the escape yields the *symbol*, and
;; `data/external-representation.scm` is where rendering is asserted deliberately.
(define-syntax make-ellipsis (syntax-rules () ((_) (quote (... ...)))))

(test-equal "an escaped ellipsis alone yields the symbol" '...
  (make-ellipsis))

;; The escape covers a whole template, so a pattern variable inside one is
;; still substituted while the `...` beside it stays a symbol.
(define-syntax escaped-with-var (syntax-rules () ((_ x) (quote (... (x ...))))))

(test-equal "and a pattern variable inside one is still substituted" '(foo ...)
  (escaped-with-var foo))

;; Nested: the outer escape protects the inner `...`, which is then an ordinary
;; symbol in the output rather than a second escape.
(define-syntax escaped-twice (syntax-rules () ((_ x y) (quote (... (... x y))))))

(test-equal "and an escape inside an escape is a symbol, not a second escape"
  '(... bar baz)
  (escaped-twice bar baz))

;; What the escape is *for*: a macro that writes a macro escapes the inner
;; macro's ellipsis so the outer expansion leaves it alone, and the generated
;; macro then uses it as an ordinary ellipsis. From
;; `compliance/macros_advanced.rs` (#193), as is the rest of this section.
;;
;; The caller names the generated macro. The Rust original had the template
;; name it `apply-to-list` itself, and then called it from outside — which
;; R7RS §4.3.2's renaming forbids and chibi and Gauche both refuse, while
;; Patina answered. That program is kept, pinned as a Patina defect, in
;; `introduced-definitions.scm`; this row keeps the claim about the ellipsis.
(test-equal "an escaped ellipsis works in the macro it generates" 1
  (let ()
    (define-syntax listify
      (syntax-rules ()
        ((listify e name)
         (define-syntax name
           (syntax-rules ()
             ((name arg (... ...))
              (e (list arg (... ...)))))))))
    (listify car apply-to-list)
    (apply-to-list 1 2 3)))

;; The same with a value the outer macro substitutes into the inner template,
;; beside the escaped ellipsis. `syntax-rules-literals.scm` has this generator
;; at top level, for the claim that a substituted macro name still matches.
(test-equal "and beside a substituted pattern variable" '(x 1 2 3)
  (let ()
    (define-syntax make-wrapper
      (syntax-rules ()
        ((make-wrapper wrapper-name tag)
         (define-syntax wrapper-name
           (syntax-rules ()
             ((wrapper-name item (... ...))
              '(tag item (... ...))))))))
    (make-wrapper wrap-with-x x)
    (wrap-with-x 1 2 3)))

;; A declared ellipsis (SRFI 46) is a *declaration*, so a binding of `...`
;; around it has no bearing on it either way. #114 looked up the spelling `...`
;; and broke this row while fixing the two above. chibi agrees; Gauche never
;; reaches it, for the reason in the header.
(test-equal "a declared ellipsis is unaffected by a binding of dots" '(2 3 1)
  (let ((... 'dots))
    (define-syntax m3 (syntax-rules ::: () ((_ a b :::) (list b ::: a))))
    (m3 1 2 3)))

;; ── Declaring the ellipsis, and `...` as a literal ──────────────────────────
;;
;; SRFI 46, adopted by R7RS §4.3.2: `(syntax-rules <ellipsis> (<literal> …) …)`
;; names the ellipsis, and a literal takes priority over the ellipsis. Without a
;; binding of dots anywhere, so these are the plain cases the row above varies.

;; `...` is both the declared ellipsis and a literal, so the literal wins and
;; the template's `...` is a symbol.
(test-equal "... in the literals list is a literal, not the ellipsis" '(100 ...)
  (let ()
    (define-syntax elli-lit-1
      (syntax-rules ... (...)
        ((_ x)
         '(x ...))))
    (elli-lit-1 100)))

(test-equal "a declared ellipsis stands in for ..." '(1 2 3)
  (let ()
    (define-syntax colon-list
      (syntax-rules ::: ()
        ((_ x :::)
         '(x :::))))
    (colon-list 1 2 3)))

;; With `:::` declared, `...` is free to be a literal, and matches only itself.
(test-equal "and frees ... to be a literal" '(a b)
  (let ()
    (define-syntax with-dots
      (syntax-rules ::: (...)
        ((_ x ... y)
         '(x y))))
    (with-dots a ... b)))

;; ── Rows Gauche cannot compile ──────────────────────────────────────────────
;;
;; Last in the file on purpose: a row appended at the end must not land inside
;; this clause by accident, where it would silently stop running on Gauche with
;; nothing to report it — the driver's floor counts Patina, where it still runs.
(cond-expand
  (gauche)   ; cannot compile the rows below — see the header
  (else
   ;; `swap-first-two` is written inside the binding, so its `...` is the
   ;; variable `dots` and the pattern binds three variables: `(list b a ...)`
   ;; puts them back in the order `b a dots`, where a real ellipsis splices.
   (test-equal "a syntax-rules written where dots is bound has no ellipsis"
     '(2 1 3)
     (let ((... 'dots))
       (define-syntax swap-first-two
         (syntax-rules () ((_ a b ...) (list b a ...))))
       (def-first first-of)
       (swap-first-two 1 2 3)))

   ;; The opposite direction **in the same scope** — the preamble is repeated
   ;; rather than shared for that reason: one body has to bind `...` and hold
   ;; both macros, which is the shape a decision made once per body fails. This
   ;; is the half a per-macro decision loses: `def-first` is written at top
   ;; level and escapes an ellipsis into what it generates. That token is an
   ;; ellipsis at the use site whatever `...` means there, so `first-of` takes
   ;; one argument and any number more.
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
   ;; and one file already carries that cost. Reporting the reference as a
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

(test-end)
