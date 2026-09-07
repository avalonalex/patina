;; A character above ASCII starts an identifier — R7RS §7.1.1 and §7.1.
;;
;; §7.1.1 builds ⟨identifier⟩ from ASCII letters, but §7.1 lets an
;; implementation extend the grammar and does not say how far. R6RS §4.2.4
;; spells out a limit by Unicode general category; chibi, Gauche and Chez all
;; go past it and read *any* character above ASCII. Patina matches them, which
;; needs no category tables and can only widen the accepted language, since
;; every token this admits used to be a lex error.
;;
;; Found via SRFI 197, whose reference implementation uses `…₁` as its custom
;; `syntax-rules` ellipsis so its templates can emit a literal `...`.
;;
;; Migrated from `crates/patina-tests/tests/unicode_identifiers.rs` (#193
;; Phase 1). 8 `#[test]` functions there over 19 assertions; **20 rows here**.
;; One assertion did not come: the non-breaking space, which is a *lex* error
;; and so is rejected before the program runs — no `guard` and no `test-error`
;; can reach it. It went to `callability.rs`, which the one-home rule gives to
;; rows about what a program cannot observe from inside itself, and it went as
;; `assert_program_eval_error_at`, which names the stage. Two rows are new:
;; `display`'s answer, below, and `write` on `“` separately from the rest.
;;
;; **This file needed `written` before it could migrate.** The writer rows are
;; about the bars, and `test-equal` with `equal?` compares symbols, which are
;; `eq?` to themselves whatever the writer does with them.
;;
;; ── Measured 2026-09-07 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   20 pass
;;   chibi                     19 pass, 1 fail
;;   Gauche                    18 pass, 2 fail
;;
;; The three failures are two differences, and the pleasing part is that each
;; oracle corroborates us on the one the other disagrees with — so neither is
;; "the" reference here:
;;
;;                        (write '→)   (display '→)
;;     Patina             |→|          →
;;     chibi              |→|          |→|
;;     Gauche             →            →
;;
;; R7RS §6.13.3 requires `write` output to read back as the same object, which
;; `|→|` and `→` both do on an implementation that accepts the bare form — so
;; all three conform, and Gauche's bare spelling is a bet that every reader
;; takes it. Ours declines that bet; chibi declines it too. §6.13.3 says
;; `display` writes strings and characters without quoting and leaves symbols
;; to the implementation, so chibi's bars there conform as well.
;;
;; Gauche's rule is category-sensitive rather than "bare above ASCII": it bars
;; `“` exactly as we do, and that row passes everywhere. Left unscoped for the
;; usual reason — a recorded difference outlives a row that vanishes, and the
;; driver runs only our two backends.

(import (scheme base) (scheme write) (srfi 64))

(define (written x) (let ((p (open-output-string))) (write x p) (get-output-string p)))
(define (displayed x) (let ((p (open-output-string))) (display x p) (get-output-string p)))

;; Top level, not inside a `let`: these are the definitions the rows are about,
;; and an internal `define` is `letrec*` on a local slot rather than the global
;; binding a bare reference resolves through.
(define λ 1)
(define café 2)
(define … 3)
(define → 4)
(define ± 5)
(define “ 6)
(define ₁ 11)
(define …₁ 42)

(test-begin "unicode-identifiers")

;; ── What the reader accepts ─────────────────────────────────────────────────

;; `λ` and `café` already worked — the ASCII rule was already backed by a
;; Unicode-aware `is_alphabetic` — and stay so a future narrowing cannot
;; quietly take letters with it.
(test-equal "a Greek letter is an identifier" 1 λ)
(test-equal "an accented Latin word is an identifier" 2 café)

;; These are what the change added: general categories Po, Sm and So.
(test-equal "category Po is an identifier" 3 …)
(test-equal "category Sm is an identifier" 4 →)
(test-equal "category So is an identifier" 5 ±)

;; The case that separates the rule we chose from the one we did not. Every
;; other character in this file is inside R6RS §4.2.4's constituent set, so a
;; narrowing to the R6RS rule would leave the rest of these rows passing. `“`
;; is category Pi, which R6RS excludes and chibi, Gauche and Chez all accept.
;; It is the reason the rule is "anything above ASCII" rather than a category
;; list, so it is the one that has to be asserted.
(test-equal "a character outside the R6RS categories is still an identifier" 6 “)

;; The shape SRFI 197 needs: a subscript digit, Unicode category No.
;; `char::is_numeric` is Unicode-aware, so before this the number dispatch
;; claimed it and reported `Invalid number: ₁` — it never reached the
;; identifier path at all.
(test-equal "a non-ASCII numeric is an identifier, not a number" 11 ₁)
(test-equal "and so is one with a non-numeric prefix" 42 …₁)
(test-assert "a non-ASCII numeric quotes as a symbol" (symbol? '₁))
(test-equal "its name survives symbol->string intact" "…₁" (symbol->string '…₁))
(test-assert "and interning the same text gives the same symbol"
  (eq? '…₁ (string->symbol "…₁")))

;; SRFI 197's actual use: a custom ellipsis, so the template can emit `...` as
;; a literal rather than as a repetition marker. Note that `…₁` is also bound
;; as a variable above — the ellipsis position is not a reference, and all
;; three implementations agree it is not.
(define-syntax my-list
  (syntax-rules …₁ ()
    ((_ x …₁) (list x …₁))))

(test-equal "a unicode identifier works as a custom ellipsis"
  '(1 2 3) (my-list 1 2 3))

;; ── What the reader still reads as a number ─────────────────────────────────
;;
;; The three ASCII forms that go through the predicates this change narrowed:
;; the dispatch itself, `peek_is_numeric` and `peek_is_decimal_start`.
;; Deliberately not a tour of number syntax — rationals, complexes, radix
;; prefixes and exponents reach `read_number` through paths the change never
;; touched, and are covered in `compliance/`.
(test-equal "a bare integer still lexes as a number" 42 42)
(test-equal "a leading decimal point still lexes as a number" 0.3 .3)
(test-equal "and so does a signed one" -0.25 -.25)

;; ── What the writer does with them ──────────────────────────────────────────
;;
;; The writer answers a different question from the reader — "would every R7RS
;; reader take this bare?" — so it stays strict and bars what it cannot vouch
;; for. See the header table: chibi agrees, Gauche does not.
(test-equal "write bars a non-ASCII identifier" "|…₁|" (written '…₁))
(test-equal "and bars a symbol-category one too" "|→|" (written '→))

;; The one all three bar, because Gauche's rule is category-sensitive where
;; ours is positional. Separate from the two above so the agreement is visible
;; rather than buried in a row that Gauche fails for an unrelated character.
(test-equal "every implementation bars the Pi-category one" "|“|" (written '“))

;; `display` is the other half of the strictness question, and the original
;; `.rs` row only checked that it did not raise. Asserting the text is what
;; found chibi answering `|→|` here — it is the one row Gauche corroborates
;; and chibi does not.
(test-equal "display writes a symbol bare, bars and all left off"
  '("→" "…₁" "“")
  (list (displayed '→) (displayed '…₁) (displayed '“)))

;; The round trip the strictness is *for*: our own output has to read back.
(test-assert "our barred output reads back as the same symbol"
  (eq? '…₁ (string->symbol (symbol->string '…₁))))

(test-end)
