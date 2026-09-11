;; `syntax-rules` literals — R7RS §4.3.2: an element of the input matches a
;; literal identifier only when both occurrences have the same lexical binding,
;; or the two are spelled alike and neither is bound.
;;
;; **Moved from `crates/patina-tests/tests/hygiene.rs`** (#193), in two slices:
;; the literal-matching tests first, then the two about `_`, which is a literal
;; the moment it appears in a literals list. That file's tests are portable
;; value assertions, but most of them built a tree-walker by hand and so never
;; ran on the VM — the default backend. These rows run on both, and under chibi
;; and Gauche.
;;
;; The rule reads as bookkeeping and is not: it is what lets a user rebind `=>`
;; or `else` and have `cond` stop treating them as syntax, and what stops a
;; macro's own binder from swallowing a user's identifier of the same name. Most
;; of the file is those two directions, asked in the places they can go wrong.
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   19 pass
;;   chibi                     19 pass
;;   Gauche                    18 pass, 1 fail — registered, and filed upstream
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
;; "An identifier substituted into a pattern is not the template's literal"
;; asserted only that the program did not error, its comment saying the result
;; "depends on hygiene semantics". All four implementations answer `bound`
;; (measured 2026-09-09), so it pins that now. It was first moved here under
;; the name "a literal reaching the pattern through a pattern variable", with a
;; comment reading the answer backwards; see the row.
;;
;; ── From `compliance/macros_advanced.rs` (#193) ────────────────────────────
;;
;; Nine rows arrived from that module's literal and identifier-identity tests,
;; and two of its tests are rows this file already had: its
;; `test_bound_identifier_equality_in_nested_macros` and
;; `test_nested_macro_literal_matching_same_symbol` were the same programs as
;; the rows named at each, down to the symbols returned in one case, so they
;; became comments there rather than copies. The last section below, on
;; identifier identity in a macro that writes a macro, is new.
;;
;; Measured 2026-09-11: patina VM and tree-walker 28 pass. chibi passes 27 and
;; differs on the row asserting a duplicated pattern variable is an error,
;; which R7RS leaves to the implementation (registered as latitude). Gauche
;; passes 27 and fails the row it already failed.
;;
;; ── Where the rest of `hygiene.rs` went ─────────────────────────────────────
;;
;; The migration is finished, in four slices. `hygiene.rs`'s 49 tests are now
;; the literal-matching rows and the two about `_` here, the `let-syntax` rows
;; in `expansion/let-syntax.scm`, the ellipsis-escape rows in
;; `expansion/ellipsis.scm`, and the capture and macro-generating-macro rows in
;; `expansion/hygiene.scm`. That file is deleted and the integration-binary
;; count went 73 to 72 with it. `hygiene_matrix.rs` is not part of
;; this — it is a 28-shape scoreboard against chibi and Racket, read as a table,
;; and stays Rust.

(import (scheme base)
        (rename (scheme base) (else alt) (=> arrow))
        (scheme eval)
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
;; regression guards for `cond`/`case` being `derived-forms.scm`'s "cond falls
;; through to else", "cond's => passes the test's value" and "case falls
;; through to else".
;; Kept here anyway, and the difference is what the row is *for*: there it would
;; be a second guard for `cond`, here it is the control that gives the shadowed
;; row above its meaning. If `cond`'s arrow breaks, three files fail and
;; `derived-forms.scm` is the one to read.
(test-equal "an unshadowed => is still the arrow" 'got-true
  (cond (#t => (lambda (x) 'got-true))))

;; `else` is the same question. Bound as a variable it is an ordinary test
;; expression — 5 is true, so the clause runs and answers 9.
;; The `else` half has a sibling that predates this file:
;; `keyword-bindings.scm`'s "a rebound else does not match" runs
;; `(let ((else #f)) (cond (else 1) (#t 2)))` — it moved from `hygiene.rs` to
;; `core_syntax_bindings.rs` when `else` became a syntactic binding, and from
;; there to the suite (#193 Phase 2). This row is the
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
;; tests became seventeen rows rather than nineteen; the two `_` rows arrived
;; later, from two more tests, which is the file's nineteen.
;;
;; In the first, `k` is bound *before* the macro is defined, so the
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
;; The inner literals list `(k)` is written in the outer *template*, so its `k`
;; is introduced and carries the outer expansion's scope. The inner pattern's
;; `x` is substituted from the use site `(m k)`, and carries none. They are
;; spelled alike but are different identifiers, so the substituted `k` is NOT a
;; literal: it is a pattern variable, it matches `z`, and the first rule
;; answers. Literal *membership* compares identity (`bound-identifier=?`);
;; were it the substituted `k` acting as a literal, the answer would be `free`.
;;
;; This row reached the file under the name "a literal reaching the pattern
;; through a pattern variable", with a comment saying the substituted `k` was
;; still a literal and `z` not it — which predicts `free`, the opposite of the
;; answer it asserted. The reading above is the one in
;; `compliance/macros_advanced.rs`'s `test_bound_identifier_equality_in_nested_macros`,
;; which was this program with `bound-identifier=?` and `free-identifier=?` as
;; the symbols, verified against Chez Scheme there. The Rust original of this
;; row asserted only that it did not error; all four implementations answer
;; `bound`.
(test-equal "an identifier substituted into a pattern is not the template's literal"
  'bound
  (let-syntax ((m (syntax-rules ()
                    ((m x) (let-syntax ((n (syntax-rules (k) ((n x) 'bound) ((n y) 'free))))
                             (n z))))))
    (m k)))

;; The same shape spelled `foo`, so the outcome follows from introduced versus
;; substituted identity rather than from anything about the name `k`.
(test-equal "and the same under another spelling" 'bound-identifier=?
  (let-syntax ((m (syntax-rules ()
                    ((m x)
                     (let-syntax ((n (syntax-rules (foo)
                                       ((n x) 'bound-identifier=?)
                                       ((n y) 'free-identifier=?))))
                       (n z))))))
    (m foo)))

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
;; so they match by the rule's second half. `compliance/macros_advanced.rs`'s
;; `test_nested_macro_literal_matching_same_symbol` was this program exactly.
(test-equal "a literal in a generated macro matches the same literal" 'matched-k
  (let-syntax ((m (syntax-rules ()
                    ((m ignored)
                     (let-syntax ((n (syntax-rules (k) ((n k) 'matched-k) ((n y) 'no-match))))
                       (n k))))))
    (m anything)))

;; And a different identifier from the same template does not match it: `z`
;; is not `k` by name, whatever the scopes.
(test-equal "and does not match a different identifier" 'no-match
  (let-syntax ((m (syntax-rules ()
                    ((m ignored)
                     (let-syntax ((n (syntax-rules (k) ((n k) 'matched-k) ((n y) 'no-match))))
                       (n z))))))
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

;; ── `_`, which is a pattern token until it is a literal ─────────────────────
;;
;; **From `hygiene.rs`** (#193). R7RS §4.3.2 gives `_` two jobs. In a pattern it
;; matches anything and binds nothing, which is why `count-args` can ask only
;; about arity. Named in the literals list it stops being a wildcard and becomes
;; an ordinary literal, matching only an identifier with the same binding — the
;; same "by binding, not by spelling" rule the rest of this file is about,
;; applied to the one token with a second job.
(define-syntax count-args (syntax-rules () ((_ a) 1) ((_ a b) 2) ((_ a b c) 3)))

(test-equal "_ is a wildcard that matches anything and binds nothing" '(1 2 3)
  (list (count-args x) (count-args x y) (count-args x y z)))

;; With `_` declared a literal, `(_ _ _)` matches only a call spelled with two
;; literal underscores; `(count-to-2_ a b)` falls through to the dotted
;; catch-all
;; instead. The macro's *own* keyword position is still matched by the first `_`
;; of each rule, which is the part that has to keep working for the rest to mean
;; anything.
(define-syntax count-to-2_
  (syntax-rules (_)
    ((_) 0)
    ((_ _) 1)
    ((_ _ _) 2)
    ((x . y) 'fail)))

(test-equal "and in the literals list it matches only itself"
  '(2 0 fail fail)
  (list (count-to-2_ _ _)
        (count-to-2_)
        (count-to-2_ a b)
        (count-to-2_ a b c d)))

;; ── Identifier identity in a macro that writes a macro ──────────────────────
;;
;; **From `compliance/macros_advanced.rs`** (#193), where every case was
;; checked against Chez Scheme. The common thread: an identifier's
;; classification inside an inner `syntax-rules` depends on its *identity* —
;; name plus scopes — and never on its name alone. An identifier substituted
;; from the outer use site and one introduced by the outer template can be
;; spelled the same and still be different identifiers.

;; A substituted identifier that is not in the inner literals list is an
;; ordinary pattern variable, so it binds whatever the inner macro is called
;; with.
(define-syntax gen-with-pattern-var
  (syntax-rules ()
    ((_ nm v) (define-syntax nm (syntax-rules () ((_ v) (list v v)))))))

(gen-with-pattern-var twice q)

(test-equal "a substituted identifier is a pattern variable" '(5 5) (twice 5))

;; A substituted identifier that *is* in the inner literals list stays a
;; literal, so it matches only itself.
(define-syntax gen-with-literal
  (syntax-rules ()
    ((_ nm k)
     (define-syntax nm
       (syntax-rules (k) ((_ k) 'got-key) ((_ x) 'other))))))

(gen-with-literal key-probe key)

(test-equal "a substituted identifier in the literals list stays a literal"
  '(got-key other)
  (list (key-probe key) (key-probe zzz)))

;; The `new-symbol?` guard from `(chibi parse)`'s `grammar-bind`, reduced.
;; `syntax-rules` has no way to compare two identifiers, so the guard builds an
;; inner macro whose literals list holds the names bound so far and calls it
;; with an identifier that matches nothing. If the name is a literal, rule 1
;; cannot match and rule 2 answers "already bound"; otherwise the name is a
;; pattern variable, rule 1 matches, and the answer is "new". A literal that
;; matched *any* identifier made the guard answer "new" every time, so the
;; same grammar nonterminal got a variable more than once — which is what
;; blocked chibi-parse and edn.
(define-syntax probe-test
  (syntax-rules ()
    ((_ name (lit ...))
     (let-syntax ((probe (syntax-rules (lit ...)
                           ((probe name sk fk) sk)
                           ((probe _ sk fk) fk))))
       (probe random-symbol-to-match 'new 'already)))))

(test-equal "chibi parse's new-symbol guard" '(already new)
  (list (probe-test space (space term))
        (probe-test other (space term))))

;; The macro-keyword position of a rule is positional, so a substituted macro
;; name still matches whatever the call spells. `expansion/ellipsis.scm` has
;; the same generator inside a body; this is the top-level form.
(define-syntax make-wrapper
  (syntax-rules ()
    ((_ wrapper-name tag)
     (define-syntax wrapper-name
       (syntax-rules ()
         ((wrapper-name item (... ...)) '(tag item (... ...))))))))

(make-wrapper wrap-with-x x)

(test-equal "a substituted macro name still matches" '(x 1 2 3)
  (wrap-with-x 1 2 3))

;; A substituted identifier is still a duplicate of *itself*. edn passes a
;; whole expression where `(chibi parse)`'s `grammar-bind` expects a name, and
;; that expression mentions `ch` twice; substituting it into the guard's
;; pattern asks for two pattern variables with one identity. Chez rejects this
;; with "duplicate pattern variable ch", and so do Patina and Gauche. R7RS
;; §4.3.2 makes it "an error" without requiring a signal, and chibi 0.12
;; accepts it (registered as latitude) — so edn depends on chibi's leniency
;; rather than on anything portable.
;;
;; Through `eval`, so the refusal is a caught error rather than a file that
;; will not compile, and inside a `let` body because `(environment …)` is
;; immutable — a top-level `define-syntax` there would fail for that reason
;; instead. The first row is the control: the same program with the two names
;; distinct runs, so the error in the second is the duplicate's.
(test-equal "a substituted pattern with distinct names is fine" 'matched
  (eval '(let ()
           (define-syntax gen
             (syntax-rules ()
               ((_ nm blob) (define-syntax nm (syntax-rules () ((_ blob) 'matched))))))
           (gen m (f ch dh))
           (m (f 1 2)))
        (environment '(scheme base))))

(test-error "a substituted identifier repeated in a pattern is an error" #t
  (eval '(let ()
           (define-syntax gen
             (syntax-rules ()
               ((_ nm blob) (define-syntax nm (syntax-rules () ((_ blob) 'matched))))))
           (gen m (f ch ch))
           (m (f 1 2)))
        (environment '(scheme base))))

;; A literal matches an input identifier when both are unbound and share a
;; name, even though one is introduced and the other substituted. §4.3.2 gives
;; literal *matching* `free-identifier=?` semantics — same binding, or both
;; unbound with the same name. That is a different question from literal
;; *membership*, which compares identity (the substituted-identifier rows
;; above). Conflating the two is what made the introduced literal `k` below
;; unable to match the substituted `k`. Chez and Gauche answer (lit notlit),
;; per the Rust original's comment.
(define-syntax unbound-k-probe
  (syntax-rules ()
    ((_ e) (let-syntax ((n (syntax-rules (k) ((n k) 'lit) ((n x) 'notlit))))
             (n e)))))

(test-equal "a literal matches when both are unbound" '(lit notlit)
  (list (unbound-k-probe k) (unbound-k-probe other)))

(test-end)
