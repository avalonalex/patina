;; `|…|` identifiers — R7RS §7.1.1, both halves: what the reader accepts inside
;; the bars, and which symbols the writer puts bars back around.
;;
;; Migrated whole from `crates/patina-tests/tests/vertical_bar_identifiers.rs`
;; (#193 Phase 1). 28 assertions there, 28 rows here.
;;
;; **This file needed `written` before it could migrate.** Most of its rows
;; assert a *printed* form — `'|.|` writes as `|.|` — and `test-equal` compares
;; with `equal?`, which would have thrown that away and left the rows asserting
;; only that a symbol is itself. `written` puts the writer back under test, so
;; the migration keeps the coverage rather than the row count:
;;
;;   (test-equal "…" "|.|" (written '|.|))
;;
;; That is the helper `docs/TEST_ORGANIZATION.md` names as the standing cost of
;; this migration, paid back here.
;;
;; 29 rows: 28 migrated, plus the round-trip invariant at the end. All agree on
;; patina VM, patina tree-walker, chibi and Gauche.
;;
;; The import set names `(scheme read)` and `(scheme write)` explicitly. Both
;; resolve without one on Patina, because the top level carries `(scheme base)`
;; and that library exports `read` and `write` (issue #211) — so an insufficient
;; import set is invisible here and fails on both oracles.

(import (scheme base) (scheme read) (scheme write) (srfi 64))

(define (written x) (let ((p (open-output-string))) (write x p) (get-output-string p)))

(test-begin "vertical-bar-identifiers")

;; ── What the bars let a name contain ────────────────────────────────────────

(define |hello world| 42)
(test-equal "a space" 42 |hello world|)

(define |foo(bar)baz| 100)
(test-equal "delimiters that would otherwise end the token" 100 |foo(bar)baz|)

(define || "empty")
(test-equal "the empty name" "empty" ||)

(define |\t\t| 999)
(test-equal "mnemonic escapes" 999 |\t\t|)

(define |foo\|bar| 777)
(test-equal "an escaped bar, inside bars" 777 |foo\|bar|)

(define (|add two numbers| a b) (+ a b))
(test-equal "a procedure name with spaces" 30 (|add two numbers| 10 20))

(test-equal "a parameter name with spaces" 15 ((lambda (|x y|) (+ |x y| 5)) 10))

;; ── Escapes name the same symbol as the characters they stand for ───────────
;;
;; Both directions, because one alone would pass if the reader simply never
;; decoded the escape: `|H\x65;llo|` and `Hello` have to be one symbol, whichever
;; spelling the `define` used.

(define |H\x65;llo| 123)
(test-equal "an escaped name is reachable by its plain spelling" 123 Hello)

(define Hello2 456)
(test-equal "…and a plain name by its escaped spelling" 456 |H\x65;llo2|)

(define |\x3BB;| "lambda symbol")
(test-equal "a hex escape above ASCII" "lambda symbol" |\x3BB;|)

;; ── Which symbols the writer bars ───────────────────────────────────────────
;;
;; The writer's question is "would every R7RS reader take this bare?", so it
;; bars anything it cannot vouch for. These rows are why this file needed
;; `written`: comparing the symbol to itself would pass whatever the writer did.

(test-equal "a name containing a space" "|hello world|" (written '|hello world|))

(test-equal "a lone dot" "|.|" (written '|.|))
(test-equal "a leading comma" "|,a|" (written '|,a|))
(test-equal "a double quote" "|\"|" (written '|"|))

;; Anything that would read back as a number has to be barred, or the writer's
;; output would not round-trip through the reader.
(test-equal "a digit" "|2|" (written '|2|))
(test-equal "a signed integer" "|+3|" (written '|+3|))
(test-equal "a decimal" "|-.4|" (written '|-.4|))
(test-equal "an imaginary unit" "|+i|" (written '|+i|))
(test-equal "a negative imaginary unit" "|-i|" (written '|-i|))
(test-equal "positive infinity" "|+inf.0|" (written '|+inf.0|))
(test-equal "negative infinity" "|-inf.0|" (written '|-inf.0|))
(test-equal "NaN" "|+nan.0|" (written '|+nan.0|))
(test-equal "NaN, in the spelling the reader also accepts" "|+NaN.0|"
  (written '|+NaN.0|))
(test-equal "something that only starts like NaN" "|+NaN.0abc|"
  (written '|+NaN.0abc|))
(test-equal "something that only starts like a number" "|123abc|"
  (written '|123abc|))

;; ── And which it leaves bare ────────────────────────────────────────────────
;;
;; The other half of the same claim. Without these, a writer that barred every
;; symbol would pass every row above.

(test-equal "an ordinary identifier needs no bars" "test" (written '|test|))
(test-equal "…nor does another" "hello" (written '|hello|))
(test-equal "hyphens are ordinary" "a-b-c" (written '|a-b-c|))

;; ── The invariant behind all of it ──────────────────────────────────────────
;;
;; Every row above pins one spelling; this pins the property that makes the
;; spellings matter. Whatever the writer emits, the reader must take back as the
;; same symbol.

(test-equal "every written symbol reads back as itself" #t
  (let loop ((syms (list '|hello world| '|.| '|,a| '|2| '|+inf.0| '|123abc|
                         '|test| '|a-b-c| '|foo(bar)baz| '||)))
    (or (null? syms)
        (and (eq? (read (open-input-string (written (car syms)))) (car syms))
             (loop (cdr syms))))))

(test-end)
