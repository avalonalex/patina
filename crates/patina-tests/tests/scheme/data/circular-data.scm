;; Datum labels — R7RS §2.4 and §6.13.3: what `read` makes of `#n=`/`#n#`, and
;; where `write` and `write-shared` put a label back.
;;
;; Migrated whole from `crates/patina-tests/tests/circular_data.rs` (#193
;; Phase 1). 21 `#[test]` functions there over 24 `assert_program_eval_to`
;; sites, 30 assertions executed — two sites sit inside a four-way loop.
;; **36 rows here** — 27 from that file, plus nine that moved in from
;; `larceny_families.rs` because this is the file about circular data: family
;; 2's three, the record-cycle row from its "Review of #112" section, and
;; families 19 and 20's five, which are the same data seen by the macro
;; expander and by the program parser rather than by `read` or the writer
;; (a section of their own, below).
;;
;; The arithmetic for the 27 is worth spelling out: the loop's eight
;; executions become four named `declines` rows plus one combined `applies` row,
;; the standalone `quote`-declines test the loop already covered is gone as a
;; duplicate, and one row is new — the portable half of the error-object claim,
;; which is what lets that bug keep any coverage at all on chibi and Gauche.
;;
;; **This file needed `written` before it could migrate.** 20 of the 27 rows
;; assert an exact *printed* form, and `test-equal` compares with `equal?`,
;; which cannot see any of what they are about: `equal?` is required to
;; terminate on circular data (R7RS §6.1) and does — measured 2026-09-07,
;; `(equal? '#0=(x . #0#) '#0=(x . #0#))` answers `#t` at once on Patina, chibi
;; and Gauche — and it answers on *structure*, so a mislabelled `#0=`, a
;; dropped label, or a lost abbreviation leaves two structurally identical
;; values that compare equal however they print. That is the #187/#189 class,
;; and it is sharpest here: the `write` and `write-shared` rows differ in
;; nothing but printing, so without `written` several of them would be the same
;; assertion twice.
;;
;; So the three writer helpers below put the writer back under test. `written`
;; is the shared one and is copied verbatim from `docs/TEST_ORGANIZATION.md`;
;; `shared` and `displayed` are the same shape over the other two procedures
;; this file is about, and exist because `write`, `display` and `write-shared`
;; drive the same two passes and only `write` used to be pinned. (`read-back`
;; below is a fourth helper, but a reader's, not a writer's.)
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   36 pass
;;   Gauche                    30 pass, 5 fail, 1 skip
;;   chibi                     31 pass, 4 fail, 1 skip
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
;; all three implementations, and so do the round-trip row and all three
;; stack-depth rows. Only 4 of the 27 rows diverge at all, so generalising the
;; difference to the file would have scoped 22 rows that need no scoping.
;;
;; Gauche's fifth failure is not that difference at all: it is the last row in
;; the expander section, where it copies a macro argument once per insertion.
;; The register carries it as latitude.
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

;; ── equal? on cycles, which the header above rests on ──────────────────────
;;
;; **Larceny family 2** (`scheme_tests/reports/larceny_triage.md`), moved here
;; from `larceny_families.rs` because this is the file about circular data.
;;
;; The header's measurement — `(equal? '#0=(x . #0#) '#0=(x . #0#))` — is the
;; easy case: same shape, same period. These are the hard one, and the reason
;; the claim is worth testing at all. Two *distinct* cyclic lists with the same
;; unrolling, period 2 against period 4: `(equal? a a)` would be fine because
;; `eq?` short-circuits, and this looped forever until 2026-08-24, when an
;; explicit worklist with a lazily allocated visited set replaced the walk.
(test-assert "equal? terminates on two distinct cyclic lists"
  (let ((a (list 1 2)) (b (list 1 2 1 2)))
    (set-cdr! (cdr a) a)
    (set-cdr! (cdr (cddr b)) b)   ; not cdddr: that is (scheme cxr), not base
    (equal? a b)))

;; The vector shape of the same defect overflowed the Rust stack rather than
;; hanging, which is why Larceny's `read` suite died instead of stalling.
(define (cyc x) (let ((v (vector x #f))) (vector-set! v 1 v) v))
(test-assert "equal? terminates on two distinct cyclic vectors"
  (equal? (cyc 1) (cyc 1)))

;; The negatives get their own row rather than riding along in a list: they are
;; the half that says termination was not bought by answering #t to everything,
;; and folded together a regression prints two lists to diff instead of naming
;; which direction `equal?` broke in.
(test-equal "and a cycle of a different shape is not equal" '(#f #f)
  (let ((a (list 1 2)))
    (set-cdr! (cdr a) a)
    (list (equal? (cyc 1) (cyc 2))     ; same shape, different contents
          (equal? a (cyc 1)))))        ; list against vector

;; **From `larceny_families.rs`'s "Review of #112" section.** `equal?` walks
;; record fields on the same worklist as pairs and vectors, so a cycle through
;; a record terminates too — the third shape of the same defect family 2 covers
;; for lists and vectors.
(define-record-type <box> (mk v) box? (v box-v box-set-v!))
;; Top level, like the `.rs` original and like this PR's sibling change in
;; `control/wind-thunk-exceptions.scm`: `docs/TEST_ORGANIZATION.md` asks
;; migrations to leave bindings where they were. The cycle here is in a record
;; *field* rather than through a variable, so the letrec* hazard the rule names
;; does not apply — but two rows of the same kind disagreeing about it inside
;; one PR is the drift the rule exists to stop.
(define self-box-a (mk #f))
(define self-box-b (mk #f))
(define listy-box (mk 1))
(box-set-v! self-box-a self-box-a)
(box-set-v! self-box-b self-box-b)
(box-set-v! listy-box (list listy-box))
(test-equal "equal? terminates through a record field cycle" '(#t #f #t #f)
  (list (equal? self-box-a self-box-b)   ; two self-cycles, same shape
        (equal? self-box-a listy-box)    ; self-cycle vs a cycle through a list
        (equal? (mk 1) (mk 1))
        (equal? (mk 1) (mk 2))))

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

;; ── Through the macro expander ──────────────────────────────────────────────
;;
;; **Larceny families 19 and 20**, moved here from `larceny_families.rs`
;; because `read` is not the only reader that meets a label. Family 19 is the
;; expander: Larceny's `base` suite hands `test` several labelled data, and
;; until 2026-08-25 the identifier scan and the scope flip walked the cycle
;; forever — the suite hung at load, before one row of it ran. Cycles reach the
;; expander only from the reader, so a pair it has already visited holds no
;; identifiers and is skipped. Family 20 is the program parser, and is the
;; middle two rows; the triage doc keeps them as separate entries and so does
;; this section, so that either can be traced back.

(define-syntax same? (syntax-rules () ((_ x y) (equal? x y))))

(test-equal "a cyclic quoted datum can be a macro argument" '(#t #f)
  (list (same? '#0=(a b . #0#) '#1=(a b a b . #1#))
        (same? '#2=(a b . #2#) '#3=(a b c . #3#))))

;; **Larceny family 20.** R7RS §2.4 scopes a label to the outermost datum it
;; appears in, so the next datum may reuse it. The parser that reads a whole program datum by datum
;; (`parse`, behind the script runner and `eval_program`) kept one label table
;; across all of them and rejected the reuse; `read` builds a fresh parser per
;; call and never had the bug. **This file is the test** — the two definitions
;; below are read by that parser, out of this source, and a regression would
;; stop the file loading rather than fail a row.
(define first-labelled '#0=(x . #0#))
(define second-labelled '#0=(y . #0#))

(test-equal "a datum label may be reused by the next datum" '(x y #t)
  (list (car first-labelled)
        (car second-labelled)
        (eq? first-labelled (cdr first-labelled))))

;; The other half of that fix: a `#0#` whose `#0=` never came was left in the
;; datum as a placeholder object rather than reported. Read from a string
;; rather than written here for the reason the row above is written here — a
;; source file containing it would not load at all.
(test-error "a reference to a label that was never defined is an error" #t
  (read (open-input-string "(y #0# z)")))

;; **Family 19 again.** The scope flip copies a macro argument pair by pair,
;; and the copy has to close on itself where the original did — a memo from the
;; first pair — rather than splice the original's tail in after some budget,
;; which lost `eq?` identity across the cycle and left `write` a shape it could
;; not print.
;;
;; The Rust original asserted three things at once and the first was its
;; control — that the *uncopied* literal is `eq?` to its own tail, before any
;; macro touches it. That one is dropped here rather than lost: it is the row
;; "the cdr of a circular literal is eq? to the pair", 200 lines above, which is
;; the same claim with nothing in the way.
(define-syntax both (syntax-rules () ((_ x) (list x x))))

(test-assert "the expander's copy of a cycle is closed on itself"
  (let* ((v '#0=(1 . #0#)) (w (car (both v)))) (eq? w (cdr w))))

;; Split from the row above rather than bundled with it, because the two
;; answers differ by implementation and only this one does: Gauche copies the
;; argument once per insertion, so its two are not `eq?`. R7RS says nothing
;; about the identity of a literal datum reaching a template twice, so that is
;; latitude — registered, and not a claim about Gauche.
(test-assert "and both insertions of one argument are the same object"
  (let ((p (both '#1=(a b . #1#)))) (eq? (car p) (cadr p))))

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
;;
;; A list rather than one `and` of four claims: the first two are an
;; abort-detector — `written` ends in `get-output-string`, so reaching the
;; comparison at all is the assertion — while the last two are ordinary value
;; claims about the object. Bundled behind `test-assert` a failure reports a
;; bare `#f` and a regression in `error-object-message` would look exactly like
;; a regression in the writer, which is the shape `data/conversion.scm` already
;; carries a comment against.
(test-equal "writing an error object with a cyclic irritant terminates"
  '(#t #t #t "boom")
  (list (string? (written cyclic-error))
        (string? (displayed cyclic-error))
        (error-object? cyclic-error)
        (error-object-message cyclic-error)))

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
;; Last in the file deliberately — but only the oracle runs benefit, and it is
;; worth being exact about which. These are the rows whose regression mode is an
;; abort rather than a failure. Run standalone, ordering last means the 26 rows
;; above still report. Under `scheme_suite.rs` it buys nothing: the driver runs
;; each file in-process and reads the SRFI 64 counts only after it returns, so
;; an abort here takes the whole test binary down and no counts are read at all.
;; The assertions are on output *length*, not text: what is under test is that
;; the writer returns. All three implementations agree on every number here.
(define (zeros n)
  (let loop ((i 0) (acc '())) (if (= i n) acc (loop (+ i 1) (cons 0 acc)))))

;; One list for the two rows below, which is the point they make together:
;; `display` and `write-shared` run the same passes over the *same* spine, and
;; only `write` used to be pinned.
(define fifty-thousand (zeros 50000))

;; 100000 digits + 99999 separators + 2 parens.
(test-equal "write on a 100000-element list does not exhaust the stack"
  200001 (string-length (written (zeros 100000))))

;; `display` and `write-shared` build the same writer and run the same two
;; passes, but only `write` used to be pinned — so a depth regression reachable
;; only under `display_mode` or `label_shared` would have passed the suite.
(test-equal "display does not either"
  100001 (string-length (displayed fifty-thousand)))

;; The harder shape on purpose: a list consed with every one of its tails, so
;; *every* tail is shared and therefore labelled. A labelled tail opens a paren,
;; which looks like it must nest — the writer counts the open parens instead of
;; recursing for them. Before that, this aborted the process above about 25_000.
(test-assert "nor does write-shared with every tail labelled"
  (let* ((xs fifty-thousand)
         (tails (let loop ((l xs) (acc '()))
                  (if (null? l) (reverse acc) (loop (cdr l) (cons l acc))))))
    (> (string-length (shared (cons xs tails))) 100000)))

(test-end)
