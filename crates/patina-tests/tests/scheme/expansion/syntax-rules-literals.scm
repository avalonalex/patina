;; `syntax-rules` literals — R7RS §4.3.2: an element of the input matches a
;; literal identifier only when both occurrences have the same lexical binding,
;; or the two are spelled alike and neither is bound.
;;
;; **Moved from `crates/patina-tests/tests/hygiene.rs`** (#193, the file's first
;; slice). That file's 49 tests are portable value assertions, but 44 of them
;; build a tree-walker by hand and so never ran on the VM — the default backend.
;; These rows run on both, and under chibi and Gauche.
;;
;; The rule reads as bookkeeping and is not: it is what lets a user rebind `=>`
;; or `else` and have `cond` stop treating them as syntax, and what stops a
;; macro's own binder from swallowing a user's identifier of the same name. Most
;; of the file is those two directions, asked in the places they can go wrong.
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   17 pass
;;   chibi                     17 pass
;;   Gauche                    16 pass, 1 fail — registered, and filed upstream
;;
;; Gauche differs on the last row only: a literal matched against an identifier
;; the *enclosing template* just bound. It answers `matched-k` where Patina,
;; chibi and Chez 10.3.0 all answer `no-match`. Filed as shirok/Gauche#1327
;; after reproducing it on Gauche master (`f582cf69e`), so the row stays
;; unscoped — a difference in an answer is exactly what the register is for.
;;
;; **Two rows were worth more after the move than before, and both say why the
;; move was worth making.**
;;
;; The nested-template row carried the comment "Verified against chibi-scheme
;; and Gauche" in Rust. Gauche has never agreed with it, as far as any
;; measurement taken here can tell. A claim like that is unfalsifiable in a
;; comment and checked on every run in a suite file — which is the argument for
;; this whole migration in one line, and the reason not to write another one.
;;
;; "A literal reaching the pattern through a pattern variable" asserted only
;; that the program did not error, its comment saying the result "depends on
;; hygiene semantics". All four implementations answer `bound` (measured
;; 2026-09-09), so it pins that now.
;;
;; ── Where the rest of `hygiene.rs` is going ─────────────────────────────────
;;
;; This is the first of three slices. The `let-syntax` rows go to
;; `expansion/let-syntax.scm`, the capture and macro-generating-macro rows to
;; `expansion/hygiene.scm`, and the underscore and ellipsis-escape rows to
;; `expansion/ellipsis.scm`; when the last leaves, `hygiene.rs` is deleted and
;; the integration-binary count drops by one. `hygiene_matrix.rs` is not part of
;; this — it is a 28-shape scoreboard against chibi and Racket, read as a table,
;; and stays Rust.

(import (scheme base)
        (rename (scheme base) (else alt) (=> arrow))
        (srfi 64))

(test-begin "syntax-rules-literals")

;; ── A use-site binding takes a literal away from the macro ──────────────────
;;
;; The classic case, and the reason the rule exists: `cond`'s `=>` is a literal
;; in its own definition, so a program that binds `=>` as a variable gets the
;; plain `(test expr …)` clause instead of the arrow form. `'ok` is returned
;; rather than called.
(test-equal "a shadowed => is not cond's arrow" 'ok
  (let ((=> #f)) (cond (#t => 'ok))))

;; The same inside a procedure body rather than at the outermost level, because
;; the resolution runs where the reference stands.
(define (shadow-in-body) (let ((=> #f)) (cond (#t => 'shadowed-ok))))

(test-equal "and the same inside a procedure body" 'shadowed-ok (shadow-in-body))

;; The control: unshadowed, `=>` is still cond's arrow and the clause calls its
;; receiver. Without this row the one above would pass on an implementation
;; where `=>` never matched at all.
;;
;; `core_syntax_bindings.rs` argues against exactly this row and is right about
;; its own file: "a second copy only splits the failure across two files", the
;; regression guards for `cond`/`case` being `compliance/derived.rs`'s
;; `test_cond_with_else`, `test_cond_with_arrow` and `test_case_with_else`.
;; Kept here anyway, and the difference is what the row is *for*: there it would
;; be a second guard for `cond`, here it is the control that gives the shadowed
;; row above its meaning. If `cond`'s arrow breaks, three files fail and
;; `derived.rs` is the one to read.
(test-equal "an unshadowed => is still the arrow" 'got-true
  (cond (#t => (lambda (x) 'got-true))))

;; `else` is the same question. Bound as a variable it is an ordinary test
;; expression — 5 is true, so the clause runs and answers 9.
;; The `else` half has a sibling that predates this file:
;; `core_syntax_bindings.rs::test_a_rebound_else_does_not_match` runs
;; `(let ((else #f)) (cond (else 1) (#t 2)))` on both backends, and moved there
;; from `hygiene.rs` when `else` became a syntactic binding. This row is the
;; other polarity — a *true* rebound `else`, so the clause is taken rather than
;; skipped — and the two together say the binding decides, not the spelling. A
;; third copy of either is what `core_syntax_bindings.rs`'s own comment warns
;; against.
(test-equal "a shadowed else is not cond's else" 9
  (let ((else 5)) (cond (#f 1) (else 9))))

(test-equal "and an unshadowed one is" 7 (cond (#f 1) (else 7)))

;; The other direction, and the one hygiene is usually named for: a literal a
;; *template* introduced keeps working when the use site binds that name. The
;; `else` inside `my-if2` came from its own definition, where `else` is
;; `(scheme base)`'s.
(define-syntax my-if2 (syntax-rules () ((_ c t e) (cond (c t) (else e)))))

(test-equal "a template's literal survives a use-site shadow" 'right
  (let ((else #f)) (my-if2 #f 'wrong 'right)))

;; A literal is matched by *binding*, not by spelling, so an identifier imported
;; under another name still matches — `alt` and `arrow` are `(scheme base)`'s
;; `else` and `=>` renamed at the import. An implementation comparing symbols
;; would answer 1 and 'no here.
(test-equal "a renamed else matches the literal it was renamed from" 42
  (cond (#f 1) (alt 42)))

(test-equal "and a renamed =>" 'one
  (cond ((assv 1 '((1 . one))) arrow cdr) (else 'no)))

;; ── Which binding, and when ────────────────────────────────────────────────
;;
;; The pair that shows the rule is about bindings rather than about order of
;; appearance. The first row is two Rust tests collapsed:
;; `test_literal_bound_before_macro_definition` and the first half of
;; `test_literal_binding_before_vs_after` were the same program down to the
;; symbol it returned. That, and the shadowed-`=>` program appearing both as a
;; tree-walker-only test and through the both-backend helper, is why fourteen
;; tests became seventeen rows rather than nineteen. In the first, `k` is bound *before* the macro is defined, so the
;; literal in the pattern and the `k` at the use site are the same binding and
;; the literal matches. In the second the use site binds `k` *after*, so they
;; are different bindings and it does not — even though every `k` is spelled
;; alike and the two programs differ only in where the `let` sits.
(test-equal "a literal matches when both occurrences share a binding" 'matched-before
  (let ((k 999))
    (let-syntax ((n (syntax-rules (k) ((n k) 'matched-before) ((n x) 'no-match))))
      (n k))))

(test-equal "and not when the use site binds it afterwards" 'no-match
  (let-syntax ((n (syntax-rules (k) ((n k) 'matched-after) ((n x) 'no-match))))
    (let ((k 999)) (n k))))

;; The same claim once more with the binding around the *use* rather than around
;; the macro, which is the shape a program actually writes.
(test-equal "a literal shadowed at the use site does not match" 'no-match
  (let-syntax ((n (syntax-rules (k) ((n k) 'matched) ((n y) 'no-match))))
    (let ((k 42)) (n k))))

;; A literal naming a *global* — `car` — matches from a nested scope. This is
;; the second half of the rule, "both unbound with the same name", except that
;; `car` is bound, in the library: the literal and the input resolve to the same
;; imported binding. It failed here once because the literal recorded the
;; definition site's scopes and the use site's identifier carried none, so a
;; macro defined inside any scope matched no global literal at all.
(test-equal "a global literal matches from a nested scope" 'matched
  (let ()
    (define-syntax m (syntax-rules (car) ((_ car) 'matched) ((_ x) 'not-matched)))
    (m car)))

;; And from the top level, which always worked — the pair is what shows the
;; answer no longer depends on the nesting.
(define-syntax m2 (syntax-rules (car) ((_ car) 'matched) ((_ x) 'not-matched)))

(test-equal "and from the top level" 'matched (m2 car))

;; ── Literals that arrive through a macro ────────────────────────────────────
;;
;; A literal that reached the pattern through a pattern variable — `k` here is
;; substituted into the `(syntax-rules (k) …)` of the generated macro — is still
;; a literal, and `z` is not it. The Rust original asserted only that this did
;; not error; all four implementations answer `bound`.
(test-equal "a literal reaching the pattern through a pattern variable" 'bound
  (let-syntax ((m (syntax-rules ()
                    ((m x) (let-syntax ((n (syntax-rules (k) ((n x) 'bound) ((n y) 'free))))
                             (n z))))))
    (m k)))

;; A macro binds `k` in its template and generates a macro with `k` as a
;; literal. The user's `k`, arriving through a pattern variable, is not the
;; template's — so it does not match, and this is the direction that stops a
;; macro's private binder swallowing a user's identifier.
(define-syntax binds-k
  (syntax-rules ()
    ((_ e) (let ((k 1))
             (let-syntax ((n (syntax-rules (k) ((n k) 'lit) ((n x) 'var))))
               (n e))))))

(test-equal "a literal a template binds is not the user's identifier" 'var
  (binds-k k))

;; Both the literal and the input come from one template and neither is bound,
;; so they match by the rule's second half.
(test-equal "a literal in a generated macro matches the same literal" 'matched-k
  (let-syntax ((m (syntax-rules ()
                    ((m ignored)
                     (let-syntax ((n (syntax-rules (k) ((n k) 'matched-k) ((n y) 'no-match))))
                       (n k))))))
    (m anything)))

;; The same program with one `let` added: the input `k` now denotes the binding
;; that `let` makes, while the literal `k` stands outside it and is unbound. One
;; bound and one not, so no match.
;;
;; **Gauche answers `matched-k`** — registered, and filed as shirok/Gauche#1327
;; after reproducing it on master. chibi and Chez 10.3.0 answer `no-match` with
;; us. Left unscoped deliberately: the register is where a difference in an
;; answer belongs, and scoping it away is how it would never have been found.
(test-equal "but not when the template binds it first" 'no-match
  (let-syntax ((m (syntax-rules ()
                    ((m ignored)
                     (let-syntax ((n (syntax-rules (k) ((n k) 'matched-k) ((n y) 'no-match))))
                       (let ((k 99))
                         (n k)))))))
    (m anything)))

(test-end)
