;; Datum labels — R7RS §2.4 and §6.13.3: what `read` makes of `#n=`/`#n#`, and
;; where `write` and `write-shared` put a label back.
;;
;; Migrated whole from `crates/patina-tests/tests/circular_data.rs` (#193
;; Phase 1). 21 `#[test]` functions there over 24 `assert_program_eval_to`
;; sites, 30 assertions executed — two sites sit inside a four-way loop.
;; **27 rows here**, and the arithmetic is worth spelling out: the loop's eight
;; executions become four named `declines` rows plus one combined `applies` row,
;; the standalone `quote`-declines test the loop already covered is gone as a
;; duplicate, and one row is new — the portable half of the error-object claim,
;; which is what lets that bug keep any coverage at all on chibi and Gauche.
;;
;; **This file needed `written` before it could migrate.** Every row but three
;; asserts a *printed* form, and `test-equal` compares with `equal?` — which on
;; a circular structure does not terminate at all, and on the rest would have
;; left the rows asserting that a list is itself. So the three helpers below put
;; the writer back under test. `written` is the shared one and is copied
;; verbatim from `docs/TEST_ORGANIZATION.md`; `shared` and `displayed` are the
;; same shape over the other two procedures this file is about, and exist
;; because `write`, `display` and `write-shared` drive the same two passes and
;; only `write` used to be pinned.
;;
;; ── Measured 2026-09-07 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   27 pass
;;   Gauche                    23 pass, 3 fail, 1 skip
;;   chibi                     22 pass, 4 fail, 1 skip
;;
;; Every failure is one difference, and it is worth naming because it is a
;; three-way spread rather than "the oracles agree and we don't":
;;
;;   **whether `(quote x)` is written back as the abbreviation `'x`**
;;
;;                    write          write-shared
;;     Patina         'x             'x
;;     Gauche         'x             (quote x)
;;     chibi          (quote x)      (quote x)
;;
;; R7RS permits either — §6.13.3 requires only that the output `read` back to an
;; equivalent datum, and `'x` and `(quote x)` are the same datum. So all three
;; conform, and Gauche's split between the two procedures is its own choice.
;;
;; Left **unscoped** rather than `cond-expand`-ed, for the reason #214, #217 and
;; #218 give: `cond-expand (patina)` would also throw away Gauche's agreement on
;; the `write` rows, and a difference that is recorded is worth more than a row
;; that vanishes. The driver runs only the two Patina backends, so these rows
;; cost nothing in CI; they are for whoever next runs the file as an oracle, and
;; the tallies above say exactly what to expect.
;;
;; Note what does **not** diverge, because the first draft of this header
;; assumed it would: the four rows where the abbreviation *declines* agree on
;; all three implementations, and so do both round-trip and all three
;; stack-depth rows. Generalising the abbreviation difference to the file would
;; have scoped eleven rows that need no scoping.
;;
;; The one scoped row is the error object's printed form, whose premise really
;; is ours: R7RS gives error objects no external representation at all, and the
;; three implementations share no syllable of one (see below).

(import (scheme base) (scheme read) (scheme write) (srfi 64))

(define (written x) (let ((p (open-output-string))) (write x p) (get-output-string p)))
(define (shared x) (let ((p (open-output-string))) (write-shared x p) (get-output-string p)))
(define (displayed x) (let ((p (open-output-string))) (display x p) (get-output-string p)))

(define (read-back text) (read (open-input-string text)))

(test-begin "circular-data")

;; ── Reading a labelled datum, and writing it back ───────────────────────────

(test-equal "write on a circular pair read from text"
  "#0=(a . #0#)" (written (read-back "#0=(a . #0#)")))

(test-equal "write on a circular list read from text"
  "#0=(a b . #0#)" (written (read-back "#0=(a b . #0#)")))

(test-equal "display labels a cycle too — it has no choice"
  "#0=(a . #0#)" (displayed (read-back "#0=(a . #0#)")))

(test-equal "write-shared on a cycle agrees with write"
  "#0=(a . #0#)" (shared (read-back "#0=(a . #0#)")))

;; ── The same labels written in *source*, which used to overflow the stack ───

(test-equal "a quoted circular literal writes back as itself"
  "#0=(a . #0#)" (written '#0=(a . #0#)))

(test-equal "a quoted circular literal has an ordinary car"
  'hello (car '#0=(hello . #0#)))

;; The structural claim underneath the printed one: `#0#` is the *same* pair,
;; not a copy that happens to print alike.
(test-assert "the cdr of a circular literal is eq? to the pair"
  (let ((x '#0=(a . #0#))) (eq? x (cdr x))))

;; ── Data with no sharing is untouched by any of it ──────────────────────────

(test-equal "an ordinary list gets no label" "(1 2 3)" (written '(1 2 3)))
(test-equal "an ordinary vector gets no label" "#(a b c)" (written '#(a b c)))

;; ── A cycle built at run time rather than read ──────────────────────────────

(test-equal "set-cdr! makes a cycle the writer labels"
  "#0=(a b . #0#)"
  (let ((x (list 'a 'b))) (set-cdr! (cdr x) x) (written x)))

;; ── Where the label goes (issue #188) ───────────────────────────────────────
;;
;; A label has to be written where the pair it names is written. Two places in
;; the writer had nowhere to put one. Every row in this section agrees on
;; Patina, chibi and Gauche, character for character.

;; `write_tagged_list_contents` could emit a back-reference to an
;; already-defined label but never a definition, so a labelled tail carried on
;; inline as if unlabelled — nothing marked it emitted, and the back-reference
;; it was waiting for could not arrive. Plain `write` on this ordinary circular
;; list grew without bound.
(test-equal "a cycle entering at an interior cdr terminates"
  "(1 . #0=(2 3 . #0#))"
  (let ((y (list 1 2 3))) (set-cdr! (cddr y) (cdr y)) (written y)))

;; The acyclic half of the same gap: two lists ending in one pair. The sharing
;; was silently dropped — `((1 3) (2 3))`, which reads back as two separate
;; tails.
(test-equal "write-shared labels a shared tail"
  "((1 . #0=(3)) (2 . #0#))"
  (let ((t (list 3))) (shared (list (cons 1 t) (cons 2 t)))))

;; The counterpart, and the reason it is here: the tail rule was widened from
;; "labelled *and* already emitted" to "labelled", so its correctness now rests
;; entirely on pass 1 not labelling shared pairs unless `write-shared` asked.
;; Without this row, that could regress into non-conforming `write` output with
;; every other row still green.
(test-equal "plain write leaves a merely-shared tail unlabelled"
  "((1 3) (2 3))"
  (let ((t (list 3))) (written (list (cons 1 t) (cons 2 t)))))

;; When the label belongs to the pair the abbreviation would elide, the
;; abbreviation declines: `'x` writes no pair for the cdr, so there is nowhere
;; to put it. Patina used to print `('(1) #0=((1)))`, whose `#0=` names a
;; *different* structure from the one inside the shorthand.
;;
;; Four rows rather than one, because the guard covers all four abbreviations
;; and naming it after `quote` makes narrowing it to that arm look reasonable.
;; These are the rows both oracles corroborate: none of the three abbreviates
;; here, so all three print the dotted form.
(define (declines head)
  (let* ((c (list (list 1))) (q (cons head c))) (shared (list q c))))

(test-equal "the quote shorthand declines when its elided pair is labelled"
  "((quote . #0=((1))) #0#)" (declines 'quote))
(test-equal "the quasiquote shorthand declines too"
  "((quasiquote . #0=((1))) #0#)" (declines 'quasiquote))
(test-equal "the unquote shorthand declines too"
  "((unquote . #0=((1))) #0#)" (declines 'unquote))
(test-equal "the unquote-splicing shorthand declines too"
  "((unquote-splicing . #0=((1))) #0#)" (declines 'unquote-splicing))

;; ── The abbreviation rows: where the three implementations part company ─────
;;
;; See the header table. Patina abbreviates under both procedures, Gauche only
;; under `write`, chibi under neither. Kept unscoped so Gauche's agreement on
;; the first row survives.

;; The reported crash: a cycle that re-enters through a quoted form. The
;; shorthand rendered `'x` with a scratch `emitted` map of its own, so the
;; recursion could not see that the enclosing datum had already defined the
;; label, defined it again, and came back round.
;;
;; Gauche writes exactly this. chibi writes `#0=(quote #0#)`.
(test-equal "a cycle through a quote form terminates"
  "#0='#0#"
  (let ((x (list 'quote 1))) (set-car! (cdr x) x) (written x)))

;; The non-crashing half of the same bug: a label defined *twice*, once outside
;; a quoted form and once inside it. `read` cannot make sense of
;; `(#0=(1) '#0=(1))` — one label, two definitions — and the sharing it was
;; recording is gone either way. Both orders, because the definition and the
;; reference swap places. Both oracles write `(quote …)` here.
(test-equal "a label crossing a quote boundary is defined once"
  "(#0=(1) '#0#)"
  (let* ((a (list 1)) (q (list 'quote a))) (shared (list a q))))

(test-equal "and once in the other order, where the reference comes first"
  "('#0=(1) #0#)"
  (let* ((a (list 1)) (q (list 'quote a))) (shared (list q a))))

;; The other side of the `declines` guard: with nothing shared, the shorthand
;; applies. All four in one row because it is one guard answering the other way;
;; the expected list names which abbreviation drifted.
(test-equal "with nothing labelled, all four abbreviations apply"
  '("'(1)" "`(1)" ",(1)" ",@(1)")
  (list (shared (list 'quote (list 1)))
        (shared (list 'quasiquote (list 1)))
        (shared (list 'unquote (list 1)))
        (shared (list 'unquote-splicing (list 1)))))

;; ── The labels are structure, not decoration ────────────────────────────────
;;
;; Writing, reading back and writing again returns identical text for every
;; shape above. A label that named the wrong pair, or a definition that went
;; missing, would survive every row above and fail this one. Portable: it
;; compares each implementation's output against itself, so all three pass it
;; despite disagreeing about abbreviation.
(define (round-trips? v)
  (let ((text (shared v))) (string=? text (shared (read-back text)))))

(test-equal "every label form round-trips through read"
  '(#t #t #t #t #t #t)
  (let* ((t (list 3))
         (y (list 1 2 3))
         (ignored-y (set-cdr! (cddr y) (cdr y)))
         (c (list (list 1)))
         (cyc (list 'quote 1))
         (ignored-cyc (set-car! (cdr cyc) cyc))
         (a (list 1))
         (q (list 'quote a)))
    (list (round-trips? (list (cons 1 t) (cons 2 t)))
          (round-trips? y)
          (round-trips? (list (cons 'quote c) c))
          (round-trips? cyc)
          (round-trips? (list a q))
          (round-trips? (list q a)))))

;; ── A cycle through an error object's irritants ─────────────────────────────
;;
;; Pass 1 labels pairs, vectors and error objects, and its rule was written out
;; once per kind — with the error object's copy missing the arm that labels a
;; *cycle*. So this got no label and pass 2 recursed until the process aborted,
;; on `(write e)`, on `(display e)`, and on the diagnostic for an uncaught
;; `(raise e)`.
;;
;; The comment that stood where the missing arm belonged argued the case was
;; impossible: `alloc_exception` takes its irritants by value and nothing
;; mutates them afterwards. True of the `Vec`, false of what is in it — an
;; irritant is an ordinary mutable pair, and `set-car!` closes the loop.
(define cyclic-error
  (let* ((xs (list 1))
         (e (guard (c (#t c)) (error "boom" xs))))
    (set-car! xs e)
    e))

;; Portable half: that writing it *returns at all* is the whole bug, and it is
;; observable without agreeing on the text. Both oracles corroborate this.
(test-assert "writing an error object with a cyclic irritant terminates"
  (and (string? (written cyclic-error))
       (string? (displayed cyclic-error))
       (error-object? cyclic-error)
       (equal? "boom" (error-object-message cyclic-error))))

;; **Scoped to Patina, because its premise is.** R7RS gives error objects no
;; external representation, and measured 2026-09-07 the three share none:
;;
;;   patina   #0=#<error-object: boom (#0#)>
;;   chibi    #0={Exception #19 user "boom" ((#0#)) #f #f #f}
;;   gauche   #<error "boom (1)">
;;
;; Gauche is the interesting one: it renders the message at `error` time, so
;; its irritant is the pre-mutation `1` and there is no cycle left to label.
;; chibi labels it as we do. Scoped rather than left failing because there is
;; no corroboration to lose — no two of them agree — and a **reported skip**
;; rather than a bare `cond-expand` so the row cannot vanish quietly.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an error object's cyclic irritant is labelled where we print it"
  "#0=#<error-object: boom (#0#)>" (written cyclic-error))

;; ── Length is not depth ─────────────────────────────────────────────────────
;;
;; Both passes used to walk the cdr chain by recursion, so depth was the list's
;; *length* and `write` on 100_000 elements overflowed the stack — which aborts
;; the process rather than raising anything a `guard` could catch. Only nesting
;; is allowed to cost a frame now.
;;
;; Last in the file deliberately. These are the rows whose regression mode is an
;; abort rather than a failure, and an abort takes everything after it with it.
;; The assertions are on output *length*, not text: what is under test is that
;; the writer returns. All three implementations agree on every number here.
(define (zeros n)
  (let loop ((i 0) (acc '())) (if (= i n) acc (loop (+ i 1) (cons 0 acc)))))

;; 100000 digits + 99999 separators + 2 parens.
(test-equal "write on a 100000-element list does not exhaust the stack"
  200001 (string-length (written (zeros 100000))))

;; `display` and `write-shared` build the same writer and run the same two
;; passes, but only `write` used to be pinned — so a depth regression reachable
;; only under `display_mode` or `label_shared` would have passed the suite.
(test-equal "display does not either"
  100001 (string-length (displayed (zeros 50000))))

;; The harder shape on purpose: a list consed with every one of its tails, so
;; *every* tail is shared and therefore labelled. A labelled tail opens a paren,
;; which looks like it must nest — the writer counts the open parens instead of
;; recursing for them. Before that, this aborted the process above about 25_000.
(test-assert "nor does write-shared with every tail labelled"
  (let* ((xs (zeros 50000))
         (tails (let loop ((l xs) (acc '()))
                  (if (null? l) (reverse acc) (loop (cdr l) (cons l acc))))))
    (> (string-length (shared (cons xs tails))) 100000)))

(test-end)
