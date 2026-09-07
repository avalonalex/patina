;; A leading `@` starts an identifier.
;;
;; R7RS §7.1.1 makes `@` a ⟨special subsequent⟩ and not an ⟨initial⟩, so a bare
;; `@` is not a conforming identifier. Patina reads it anyway: no conforming
;; program can contain a bare `@` token, so accepting it only widens the
;; accepted language, and the third-party ecosystem depends on it — SXML's `@`
;; attribute marker and `@raw` tag, `(chibi match)`'s `@` record pattern. Chez,
;; Gauche and chibi all read it; Chez and Gauche still *write* it escaped, as we
;; do.
;;
;; Migrated whole from `crates/patina-tests/tests/at_identifiers.rs` (#193
;; Phase 1). 11 assertions there, 11 rows here.
;;
;; **chibi writes `@` bare**, where Patina and Gauche both bar it — measured
;; 2026-09-07, and the one difference in this file. The three rows below that
;; assert the written form are left unscoped for the reason #214 and #217
;; settled: `cond-expand (patina)` would also drop Gauche's agreement, which is
;; the more useful half. chibi reports exactly those three.
;;
;;   patina VM / tree-walker   11 pass
;;   Gauche                    11 pass
;;   chibi                      8 pass, 3 fail — it writes `@` rather than `|@|`
;;
;; `(scheme read)` and `(scheme write)` are named explicitly: both resolve
;; without an import on Patina, because the top level carries `(scheme base)`
;; and that library exports them (issue #211), so an insufficient import set is
;; invisible here and fails on both oracles.

(import (scheme base) (scheme read) (scheme write) (srfi 64))

(define (written x) (let ((p (open-output-string))) (write x p) (get-output-string p)))

(test-begin "at-identifiers")

;; ── The reader takes it ─────────────────────────────────────────────────────

(test-equal "a bare @ is a symbol" #t (symbol? '@))
(test-equal "…and the same symbol string->symbol makes" #t
  (eq? '@ (string->symbol "@")))

(test-equal "@ may also begin a longer name" "@raw" (symbol->string '@raw))
(test-equal "which is a different symbol" #f (eq? '@ '@raw))

;; ── The writer bars it ──────────────────────────────────────────────────────
;;
;; Asserted through `written` rather than by comparing symbols: the claim is
;; about the spelling, and `equal?` on two `@` symbols would hold whatever the
;; writer emitted. These are the three rows chibi answers differently.

(test-equal "a bare @ writes as |@|" "|@|" (written '@))

;; The shape SXML actually uses: an attribute list tagged with `@`.
(test-equal "@ inside a datum" "|@|"
  (written (car (cadr '(elem (@ (href "x")) "text")))))

;; `,@` is lexed before identifiers, so the two never compete for the same `@`.
;; That `,@` alone still splices is a lexer-level fact, pinned by
;; `patina-frontend/src/lexer/mod.rs`'s
;; `test_unquote_splicing_still_wins_over_at_identifier`; what needs saying here
;; is that both spellings survive in one form.
(test-equal "@ and unquote-splicing in one form" "(|@| 2 3)"
  (written `(@ ,@(list 2 3))))

;; The invariant behind reading `@` but writing `|@|`: our own output must read
;; back as the same symbol. Stated as the property, so narrowing whatever the
;; writer uses to decide cannot satisfy it while breaking the round trip.
(test-equal "what we write reads back as @" #t
  (eq? (read (open-input-string (written '@))) '@))

;; ── It is an ordinary identifier ────────────────────────────────────────────

;; As a `syntax-rules` literal, which is how `(chibi match)` writes its
;; named-record-field pattern. **Before the top-level `define` below**: a
;; literal matches when the pattern's and the form's identifiers have the same
;; binding, so binding `@` at the top level would change what this row tests.
(define-syntax m
  (syntax-rules (@)
    ((_ (@ x)) (list 'at x))
    ((_ (y x)) (list 'other x))))
(test-equal "@ as a syntax-rules literal" '((at 1) (other 2))
  (list (m (@ 1)) (m (z 2))))

(test-equal "@ as a let-bound variable" 6 (let ((@ 5)) (+ @ 1)))

;; Last in the file, deliberately — see the note on the row above.
(define (@ x) (* x 2))
(test-equal "@ as a top-level procedure name" 42 (@ 21))

(test-end)
