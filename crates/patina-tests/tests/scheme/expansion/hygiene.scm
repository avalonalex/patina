;; Hygiene — R7RS §4.3.2: a template's free identifier means what it meant
;; where the macro was *written*, and a binding a template introduces is
;; renamed throughout its scope.
;;
;; **Assembled from two migrations**, both under #193:
;;
;;   - `larceny_families.rs`, families 15, 36, 37, 38 and 39 — fourteen Rust
;;     tests, seventeen rows, the defects Larceny's R7RS suites surfaced;
;;   - `hygiene.rs`, its capture and macro-generating-macro tests — eighteen
;;     tests, eighteen rows, the everyday statements of the same rule.
;;
;; Thirty-two tests, thirty-five rows. **`hygiene.rs` is deleted**: it was 49
;; tests of which 44 built a tree-walker by hand and so never ran on the VM,
;; the default backend, and they are now spread across this file,
;; `syntax-rules-literals.scm`, `let-syntax.scm` and `ellipsis.scm` — running
;; on both backends and under chibi and Gauche. The integration-binary count
;; drops by one with it.
;;
;; `hygiene_matrix.rs` is not part of that and stays Rust: 28 shapes scored
;; against chibi and Racket, read as a table when a hygiene fix moves a row,
;; which is a different instrument from a suite.
;;
;; **Closing that gap was measured, not assumed.** Before the first slice, the
;; 46 programs held by `hygiene.rs`'s 44 hand-built-interpreter tests were run
;; on both backends and answered identically (2026-09-09) — so the rows had not
;; been hiding a VM defect, and moving them bought permanent coverage rather
;; than a fix. What the oracles then found is a different matter, and is in
;; `syntax-rules-literals.scm`'s header: a row that asserted only "did not
;; error", and one whose comment claimed Gauche agreed with it when Gauche
;; never has (shirok/Gauche#1327).
;;
;; **Three files hold claims adjacent to these, and a fourth copy is what
;; `core_syntax_bindings.rs`'s own comment warns against.** That file has the
;; rebound-`else` row, `compliance/derived.rs` the unshadowed regression guards
;; for `cond`/`case`, and `syntax-rules-literals.scm` the literal-matching rows
;; including the other polarity of the `else` case. Check those before adding a
;; row here about a keyword being shadowed.
;;
;; The first row below — a macro's `temp` not capturing the caller's — is the
;; test issue #12 was opened for, and that issue is where the original defect
;; and its reasoning live.
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   35 pass
;;   chibi                     35 pass
;;   Gauche                    35 pass
;;
;; **Nothing here diverges**, which is worth saying for thirty-five hygiene
;; rows. Most of the first seventeen were live defects in Patina within the
;; last month; three were not, and the difference matters when reading them.
;; Family 36's parameter and `let` rows and family 39's row all pass *before*
;; the fixes they document — the first two because nothing had pinned the shape
;; until PR #138 broke it with the suite staying green, the third because the
;; right answer was arriving for the wrong reason. Their comments say which is
;; which. The eighteen from `hygiene.rs` were regression guards throughout, and
;; all four implementations answered every one identically when they moved.
;; The register has no entry for this file.
;;
;; One row of family 15 is **not** here. It is the same claim as the first row
;; below with `...` as the keyword instead of `if`, and it lives in
;; `expansion/ellipsis.scm` — Gauche rejects a `syntax-rules` written where
;; `...` is bound, so keeping it here would cost Gauche this whole file.

(import (scheme base) (srfi 64))

(test-begin "hygiene")

;; ── A definition-site local that is spelled like a keyword ──────────────────
;;
;; **Larceny family 15**, one of the pair that gated Larceny's `base` at load.
;; The macro is defined *inside* the binding, so its template's free `if` is
;; that local variable — the definition scopes say so — and a template may
;; refer to it. Patina reported it as a keyword and rejected the definition.
;;
;; What made it hard is that the opposite case looks identical by spelling: an
;; *outer* macro's `if` must stay the special form even where the use site
;; binds `if`, which the third element pins. Both now fall out of one
;; resolution — a local variable is an ordinary binding, and each reference
;; resolves in the scopes it stands in — where before a spelling set vetoed
;; the check for one and could not see the other. Fixed 2026-08-25.
(define-syntax my-if (syntax-rules () ((_ c a b) (if c a b))))

;; `...` is bound here as well as `if`, though nothing below refers to it. The
;; Rust original bound both in one `let` and defined a macro per spelling, so
;; one body had to serve two keyword-spelled locals — the shape that defeats a
;; decision made once per body, which is how #111 and #114 each got one case
;; and lost another. The `...` macro is in `expansion/ellipsis.scm`; the
;; binding stays here so this row keeps the body it was written for. Measured:
;; Gauche accepts a `syntax-rules` written in this scope as long as no template
;; *uses* an ellipsis, which is why this costs the file nothing.
(test-equal "a template may refer to a definition-site local spelled like a keyword"
  '((2 nineteen) outer-if-is-syntax)
  (let ((... 'dots) (if 'nineteen))
    (define-syntax mention-if (syntax-rules () ((_ a) (list a if))))
    (list (mention-if 2) (my-if #t 'outer-if-is-syntax 'no))))

;; The generated direction: a macro defined at top level generates one used
;; inside `(let ((if …)) …)`. The generated template's `if` came from the
;; generator, where `if` is the special form, and must stay so. #114 captured
;; it.
(define-syntax def-mid
  (syntax-rules ()
    ((_ name) (define-syntax name (syntax-rules () ((_ c) (if c 'yes 'no)))))))

(test-equal "a generated macro keeps its keywords inside a binding of that name"
  'yes
  (let ((if 'shadowed)) (def-mid mid5) (mid5 #t)))

;; The same, with the binding coming from a `define` shorthand parameter rather
;; than a `let` — the case with no `let` to give it a scope. Taking the
;; enclosing scopes unchanged left the set *empty* at top level, and an empty
;; scope set is not a narrow scope but no scope at all: `insert_scoped` routes
;; it to a plain `define`, so the marker for the parameter became a
;; name-visible global that shadowed the special form for every reference,
;; macro-introduced ones included. The `fv` half is the same fault reached
;; through an internal definition instead of a parameter. One generator serves
;; both rows — they differ only in where the shadowing binding comes from, and
;; a second copy of it would obscure that.
(def-mid mid)

(define (shorthand-param if) (mid #t))
(define (internal-define) (define if 1) (mid #t))

(test-equal "a generated macro keeps its keywords under a shorthand parameter"
  '(yes yes)
  (list (shorthand-param 'shadowed) (internal-define)))

;; The direction hygiene is usually named for: a binder at the *use site* does
;; not capture a template's keyword. A parameter named `if`, and a template
;; that introduces its own `(let ((if 1)) …)` around another macro's `if`.
;; #114 broke both.
(define-syntax my-if6 (syntax-rules () ((_ c a b) (if c a b))))
(define (param-named-if if) (my-if6 #t 'ok 'no))
(define-syntax user7 (syntax-rules () ((_ e) (if #t e 'b))))
(define-syntax wrap7 (syntax-rules () ((_ e) (let ((if 1)) (user7 e)))))

(test-equal "a use-site binder does not capture a template's keyword"
  '(ok a)
  (list (param-named-if 1) (wrap7 'a)))

;; ── A use-site binding does not capture a template's reference ──────────────
;;
;; **Larceny family 36**, closed in two steps: the by-name fallback no longer
;; resurrects a binding that set-of-scopes resolution rejected
;; (`Environment::get_scoped_fallback`), and internal defines carry the scopes
;; of the body they stand in to the runtime on both backends
;; (`CpsTransformer::define_scopes`, the VM's `body_define_bindings`) — like
;; parameters, one cell reachable by name and by scopes. Every row here is a
;; regression guard now.
;;
;; The loud version: the tree-walker used to error, `Not a procedure:
;; #<integer>`, because the template's `list` fell back by name onto the
;; use-site binding after scope resolution had rejected it.
(define-syntax mk (syntax-rules () ((_ a b) (list a b))))

(test-equal "a use-site local does not capture a template's reference" '(1 2)
  (let ((list 1)) (mk 1 2)))

;; The same defect, silent instead of loud — and the plainest statement of
;; §4.3.2 there is. chibi, Racket and both backends answer 1; the tree-walker
;; answered 5 until the by-name fallback learned to skip a binding that
;; resolution rejected.
(define resolved-past 1)
(define-syntax read-it (syntax-rules () ((_) resolved-past)))

(test-equal "a template's reference resolves past a same-named use-site let" 1
  (let ((resolved-past 5)) (read-it)))

;; A same-named **parameter** does not capture a macro-introduced `set!`, and
;; nor does a `let` binder. Both are regression guards rather than defect pins:
;; PR #138 broke exactly these shapes while every test in the repo stayed
;; green, which is why they are pinned at all. The direction they guard is the
;; one family 38's rows do not: a macro defined *outside* the frame must not
;; reach *inward* to a same-named binder.
;;
;; `(list r seen)` rather than a bare call, and the call forced into a
;; definition first: R7RS §4.1.3 leaves operand evaluation order unspecified,
;; so observing both the binder's value and the global's needs the write to
;; have happened before the list is built.
(define seen 'global)
(define-syntax bump-seen (syntax-rules () ((_) (set! seen 99))))
(define seen-from-param ((lambda (seen) (bump-seen) seen) 5))

(test-equal "a parameter does not capture a macro-introduced assignment" '(5 99)
  (list seen-from-param seen))

(define noticed 'global)
(define-syntax bump-noticed (syntax-rules () ((_) (set! noticed 99))))
(define (noticed-in-let) (let ((noticed 5)) (bump-noticed) noticed))
(define noticed-from-let (noticed-in-let))

(test-equal "nor does a let binder" '(5 99) (list noticed-from-let noticed))

;; An **internal define** is the same question one syntax deeper, and it used
;; to capture on *both* backends — wider than this family was recorded as
;; covering. `read-tally`'s `tally` means the global by §4.3.2 referential
;; transparency; chibi and Racket both answer `global`, and Patina answered 5
;; until internal defines carried the scopes of the body they stand in: with
;; no scopes the define was reachable by name from the template's fallback on
;; the tree-walker, and a universal candidate on the VM.
;;
;; Ending at `(count)` would answer `global` whether the template's reference
;; reached the global — correct — or something stranger happened; observing
;; both distinguishes a clean resolution from a clobber.
(define tally 'global)
(define-syntax read-tally (syntax-rules () ((_) tally)))
(define (count) (define tally 5) (read-tally))
(define tally-read (count))

(test-equal "an internal define does not capture a template's reference"
  '(global global)
  (list tally-read tally))

;; The same shape through a macro-introduced `set!`: the write must reach past
;; the internal define to the global. The VM captured the define until it
;; carried scopes; the tree-walker was right beforehand only by accident — its
;; scoped write found no candidate and fell back at the *root*, which is where
;; the intended target happens to live. PR #138 moved that fallback inward and
;; landed on the VM's wrong answer, which is why this pin exists.
(define ledger 'global)
(define-syntax bump-ledger (syntax-rules () ((_) (set! ledger 99))))
(define (post) (define ledger 5) (bump-ledger) ledger)
(define ledger-local (post))

(test-equal "a macro-introduced assignment reaches past an internal define"
  '(5 99)
  (list ledger-local ledger))

;; The last surface: a macro defined *inside* the frame assigns to the internal
;; define its own read can see. `bump`'s template carries the body's scopes and
;; must reach that `c` — chibi and the VM answered 15 while the tree-walker
;; said "Undefined variable": with the define bound by name only, the scoped
;; write found no candidate anywhere, recursed to the root, and found nothing
;; there either. (`get`'s fallback starts at the reading frame; `set`'s
;; terminal is the root — the asymmetry family 38 records.)
(define (bump-beside)
  (define counter 5)
  (define-syntax bump (syntax-rules () ((_) (set! counter (+ counter 10)))))
  (bump)
  counter)

(test-equal "an introduced macro can assign to the internal define beside it"
  15 (bump-beside))

;; ── A binding a template introduces is renamed through its scope ────────────
;;
;; **Larceny family 37**, and R7RS §4.3.2's own words: "if a macro transformer
;; inserts a binding for an identifier (variable or keyword), the identifier
;; will in effect be renamed throughout its scope". Three rows, because the
;; three binding forms took three different routes to it: a `let-syntax`
;; keyword got there first (family 33's second round put the form's scope on
;; its body as written), while `lambda` and `let` still bound through
;; `with_shadowed_names`, at `current_scopes + fresh`, which a reference the
;; same template introduced never carries.
;;
;; **The Rust row carried a stale quarantine note**, and this is where it goes:
;; it said the expectation was the wrong answer and that converging on chibi's
;; would be the fix landing. The fix had landed — measured 2026-09-09, all four
;; (both backends, chibi, Gauche) answer `(inner-keyword 101 201)`, which is
;; what the row already asserted. These are regression guards, not pins.
(define-syntax p (syntax-rules () ((_ x) 'outer-macro)))

(define-syntax gen-keyword
  (syntax-rules ()
    ((_) (let-syntax ((p (syntax-rules () ((_ y) 'inner-keyword)))) (p 1)))))

(define-syntax gen-lambda
  (syntax-rules () ((_) ((lambda (p) (p 1)) (lambda (y) (+ y 100))))))

(define-syntax gen-let
  (syntax-rules () ((_) (let ((p (lambda (y) (+ y 200)))) (p 1)))))

(test-equal "a macro-introduced keyword binding renames its scope"
  'inner-keyword (gen-keyword))

(test-equal "and so does a macro-introduced lambda parameter" 101 (gen-lambda))

(test-equal "and a macro-introduced let binder" 201 (gen-let))

;; ── A scoped write reaches what its own read can ────────────────────────────
;;
;; **Larceny family 38.** `Environment::get_with_scopes` resolves by subset —
;; the largest binding scope set contained in the reference's —  while
;; `set_with_scopes` demanded an *exact* match, so a reference could read a
;; binding it could not write, and the write fell through to the root's by-name
;; `set`. The `set!` here is introduced by the *inner* macro, so it carries
;; that macro's definition scopes on top of the binder's: a strict superset,
;; ordinary under set-of-scopes and fatal under exact matching.
;;
;; Fixed 2026-08-27, and only after two other things were: writes resolve the
;; way reads do, which was unsafe while a source-written parameter lived in
;; *two* cells — a by-name one and a scoped one — because the write reached the
;; scoped twin and left the by-name read stale.
(define-syntax gen-bumper
  (syntax-rules ()
    ((_ mac)
     (let ((v 1))
       (define-syntax mac (syntax-rules () ((_) (set! v (+ v 10)))))
       (mac)
       v))))

(test-equal "an introduced macro can assign to an introduced binding"
  11 (gen-bumper bump))

;; The same defect through a *source-written* binder, which is the shape that
;; made the naive fix unsafe: with two cells this answered 1 instead of 101,
;; with no error at all.
(define (add-hundred x)
  (define-syntax bump-x (syntax-rules () ((_) (set! x (+ x 100)))))
  (bump-x)
  x)

(test-equal "an introduced macro can assign to a source-written binder"
  101 (add-hundred 1))

;; ── A binder is scoped by where it stands ───────────────────────────────────
;;
;; **Larceny family 39.** `m1` is defined inside the outer `let`, so its
;; template's `x` means that binding wherever the macro is used; `m2`, one
;; `let` deeper, means the middle one.
;;
;; The answer was right before the fix and produced for the wrong reason: each
;; `let` bound its variable at a *single* fresh scope, so `{outer}` and
;; `{middle}` were unordered, neither more specific, and set-of-scopes
;; resolution could not choose — Flatt's rule calls such a reference ambiguous
;; and Racket raises. The winner came from the candidate walk visiting inner
;; environments first, which is lexical nesting and outside the model.
;;
;; Fixed 2026-08-27: a parameter written in source is bound at the scopes it
;; *stands in* — every scope enclosing the form, plus the one minted for it —
;; so nested binders form a chain, each strictly containing the last, and a
;; chain is always decidable. That is what let the ambiguity check stop being
;; opt-in, and it is why this row would now *fail* rather than answer if the
;; fix regressed.
(test-equal "a binder is scoped by where it stands" '(outer middle)
  (let ((x 'outer))
    (let-syntax ((m1 (syntax-rules () ((m1) x))))
      (let ((x 'middle))
        (let-syntax ((m2 (syntax-rules () ((m2) x))))
          (let ((x 'inner))
            (list (m1) (m2))))))))

;; ── What a template introduces is the template's, not the use site's ────────
;;
;; **From `hygiene.rs`** (#193, its last slice). The rows above are the defects
;; Larceny's suites surfaced; these are the everyday statements of the same
;; rule, and several are the textbook cases a Scheme implementation is expected
;; to get right on day one. They ran on the tree-walker alone for as long as
;; that file existed. Its other three quarters are in
;; `syntax-rules-literals.scm`, `let-syntax.scm` and `ellipsis.scm`.
;;
;; `temp` is the canonical example: the macro binds one, the use site binds one,
;; and the body the user passed in must see *theirs*.
(define-syntax my-let (syntax-rules () ((_ x body) (let ((temp x)) body))))

(test-equal "a macro-introduced binding does not capture the use site's" 5
  (let ((temp 5)) (my-let 10 temp)))

;; The same through a macro that both binds and assigns: `my-swap`'s own `temp`
;; is invisible to the caller's, which keeps its 999.
(define-syntax my-swap
  (syntax-rules () ((_ a b) (let ((temp a)) (set! a b) (set! b temp)))))

(test-equal "nor does one a macro uses for a swap" '(2 1 999)
  (let ((x 1) (y 2) (temp 999)) (my-swap x y) (list x y temp)))

;; Two at once, so the answer distinguishes "the macro's bindings won" (3) from
;; "the use site's won" (303) from correct (6).
(define-syntax with-temps
  (syntax-rules () ((_ e) (let ((temp1 1) (temp2 2)) (+ temp1 temp2 e)))))

(test-equal "and several introduced bindings stay distinct at once" 6
  (let ((temp1 100) (temp2 200)) (with-temps 3)))

;; A template's `if` is the special form even where the use site binds `if` as a
;; variable. The mirror of the first row in this file, which is the case of a
;; template referring to a definition-site *local* spelled like a keyword.
(define-syntax my-cond (syntax-rules () ((_ test then) (if test then #f))))

(test-equal "a use-site variable does not capture a template's special form"
  'success
  (let ((if 'captured)) (my-cond #t 'success)))

;; A `lambda` a template introduces binds its own `x`, and the free `n` in its
;; body is the macro's argument rather than the use site's `n` of 200.
(define-syntax make-adder-l (syntax-rules () ((_ n) (lambda (x) (+ x n)))))

(test-equal "a template's lambda binds its own parameter" 8
  (let ((x 100) (n 200)) ((make-adder-l 5) 3)))

;; What a pattern variable carries is the user's expression, unrenamed: `double`
;; means the procedure they defined.
(define-syntax apply-twice (syntax-rules () ((_ f x) (f (f x)))))
(define (double x) (* 2 x))

(test-equal "a pattern variable delivers the user's own identifier" 12
  (apply-twice double 3))

;; Renaming stops at `quote`. A symbol in a template's quoted datum is that
;; symbol, not a renamed one that would compare unequal to the user's.
(define-syntax make-symbol (syntax-rules () ((_) 'temp)))

(test-equal "a quoted symbol in a template is not renamed" #t
  (eq? (make-symbol) 'temp))

;; Recursion multiplies the introduced binding: each expansion of `nested-let`
;; makes its own `temp`, and 1 + 2 + 3 + 0 only comes out if none of them
;; captures another.
(define-syntax nested-let
  (syntax-rules ()
    ((_ () body) body)
    ((_ (val) body) (let ((temp val)) (+ temp body)))
    ((_ (val . rest) body) (let ((temp val)) (nested-let rest (+ temp body))))))

(test-equal "each expansion of a recursive macro gets its own binding" 6
  (nested-let (1 2 3) 0))

;; ── Macros that write macros ────────────────────────────────────────────────
;;
;; **From `hygiene.rs`** (#193). A template containing `define-syntax` has to
;; escape its inner ellipsis with `(... …)` — that is `expansion/ellipsis.scm`'s
;; subject — and the keyword it defines has to land where the caller can reach
;; it. These rows are about the second half.
(define-syntax gen-const-macro
  (syntax-rules ()
    ((_ name value) (... (define-syntax name (syntax-rules () ((name) value)))))))

(gen-const-macro answer 42)

(test-equal "a macro can define a macro the caller names" 42 (answer))

(define-syntax be-like-begin
  (syntax-rules ()
    ((_ name)
     (define-syntax name (... (syntax-rules () ((name e ...) (begin e ...))))))))

(be-like-begin sequence)

(test-equal "and one that takes any number of arguments" 4 (sequence 1 2 3 4))

(define-syntax gen-list-macro
  (syntax-rules ()
    ((_ name) (... (define-syntax name (syntax-rules () ((name x ...) (list x ...))))))))

(gen-list-macro listify)

(test-equal "and one whose template splices them" '(1 2 3 4 5) (listify 1 2 3 4 5))

;; The same inside a body rather than at top level, which is where the generated
;; `define-syntax` is an *internal* definition. The `(let () …)` is the test,
;; not scaffolding: at top level these would exercise a different path.
(test-equal "a macro defined in a body can define a macro there" 'hello
  (let ()
    (define-syntax foo
      (syntax-rules ()
        ((_ bar y) (define-syntax bar (syntax-rules () ((bar x) 'y))))))
    (foo my-bar hello)
    (my-bar 1)))

;; The generated keyword is named `bar`, which is also the generator's own
;; pattern variable, and the quoted `x` is the other one. If either leaked, this
;; answers something else or fails to expand.
(test-equal "even when the names collide with the generator's own" 'x
  (let ()
    (define-syntax foo
      (syntax-rules ()
        ((_ bar y) (define-syntax bar (syntax-rules () ((bar x) 'y))))))
    (foo bar x)
    (bar 1)))

(test-equal "and two generated macros stay distinct" 30
  (let ()
    (define-syntax make-const
      (syntax-rules ()
        ((_ name value) (define-syntax name (syntax-rules () ((name) value))))))
    (make-const ten 10)
    (make-const twenty 20)
    (+ (ten) (twenty))))

;; A generated template's free `base` is the one visible where the *generator*
;; was written — 100 — while `offset` came through a pattern variable. 115 is
;; the only answer that has both right.
(test-equal "a generated template reaches its definition site's binding" 115
  (let ()
    (define base 100)
    (define-syntax make-adder
      (syntax-rules ()
        ((_ name offset)
         (define-syntax name (syntax-rules () ((name x) (+ base offset x)))))))
    (make-adder add10 10)
    (add10 5)))

;; Not only macros: a template can define a *procedure* whose parameter is
;; introduced, and the caller's name for it is what binds.
(test-equal "a macro can define a procedure" 25
  (let ()
    (define-syntax def-square
      (syntax-rules () ((_ name) (define (name x) (* x x)))))
    (def-square my-square)
    (my-square 5)))

;; A generated `let-syntax`, where the keyword is a pattern variable and the
;; body is another. `expansion/let-syntax.scm` holds what such a form does to
;; the *scope* of its transformers; this is the plain case working at all.
(define-syntax make-local-const
  (syntax-rules ()
    ((_ name val body) (let-syntax ((name (syntax-rules () ((name) val)))) body))))

(test-equal "a macro can generate a let-syntax the caller uses" 43
  (make-local-const answer2 42 (+ (answer2) 1)))

;; Two expansions of one macro, each introducing `tmp`, feeding a third that
;; binds both as `lambda` parameters. If the two `tmp`s were the same identifier
;; the `lambda` would have duplicate formals; `(1 2)` is the proof they are not.
(define-syntax bind-tmp (syntax-rules () ((_ (k ...) v) (k ... (tmp . v)))))

(define-syntax through-template
  (syntax-rules () ((_ k v) (let-syntax ((go (syntax-rules () ((go) (k v))))) (go)))))

(define-syntax done
  (syntax-rules () ((_ (a b)) ((lambda (a b) (list a b)) 1 2))))

(test-equal "two expansions of one template introduce distinct identifiers"
  '(1 2)
  (bind-tmp (bind-tmp (through-template done)) ()))

;; ── One expansion's private global, seen from another (Larceny family 40) ───
;;
;; Migrated from `crates/patina-tests/tests/backend_divergence.rs` (#193),
;; where they were the three `assert_divergence` quarantines pinning **the VM**
;; as the diverging backend. Each row is a `test-error` — the right answer is a
;; refusal — and the line above it says the VM is expected to fail it by
;; answering, using the feature identifier the VM advertises (see
;; `docs/TEST_ORGANIZATION.md`). The driver fails the run the day the VM
;; starts refusing, and the fix is to delete the line and close family 40 in
;; `scheme_tests/reports/larceny_triage.md`.
;;
;; One expansion's `(define hidden-x …)` introduces a *scoped* top-level
;; definition; a different expansion's template reference to that spelling
;; carries scopes that reject it. Measured 2026-09-09 on the first shape, and
;; the VM is alone: chibi 0.12 errors "undefined variable", Gauche 0.9.15
;; errors "unbound variable", the tree-walker errors "Undefined variable", and
;; only the VM answers 10. One expansion's private definition is not another
;; expansion's to see, and three implementations say so. The VM still
;; answers: its compiler installs a bare-name alias for a renamed
;; macro-introduced global (`alpha_rename`'s `rename_body`), the mechanism
;; whose by-name reach `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 already records
;; as undecidable-under-renaming — the jabberwocky-steal defect.
;;
;; The unusual direction is the reason to read these before "fixing" one:
;; closing them means fixing relinking-by-name, not loosening the tree-walker
;; back to the capture chibi rejects.
;;
;; The definitions are top-level forms, as in the original programs; only the
;; use is inside the row, so that the refusal is a caught error and not a dead
;; file. The hidden names are `hidden-…` rather than the originals' `x`,
;; `count` and `priv` because this file already binds `count` at top level,
;; and a visible binding of the same spelling is exactly what would make the
;; reference resolve — for the wrong reason — on every implementation.

;; Read direction: `use-x`'s template `hidden-x` means whatever `hidden-x` is
;; at `use-x`'s definition site — and the only one there is `def-x`'s
;; hygienically hidden one, which chibi and the tree-walker refuse to let it
;; see.
(define-syntax def-x (syntax-rules () ((_) (define hidden-x 10))))
(def-x)
(define-syntax use-x (syntax-rules () ((_) hidden-x)))
(cond-expand (patina-vm (test-expect-fail 1)) (else))
(test-error "one expansion's definition is not another expansion's reference" #t
  (use-x))

;; Write direction, exercising `set_scoped_terminal`'s refusal — the only row
;; that reaches it, since every hygiene-matrix write row's global is a plain
;; `define` the terminal's `local_slot` arm answers first.
(define-syntax defc (syntax-rules () ((_) (define hidden-count 0))))
(defc)
(define-syntax inc (syntax-rules () ((_) (set! hidden-count (+ hidden-count 1)))))
(cond-expand (patina-vm (test-expect-fail 1)) (else))
(test-error "one expansion's definition is not another expansion's write target" #t
  (inc))

;; The generated-getter idiom across two expansions: `defgetter`'s template
;; `hidden-priv` resolves at `defgetter`'s definition site, where no visible
;; `hidden-priv` exists — `defpriv`'s is hygienically hidden. The R7RS suite's
;; `jabberwocky` shape keeps working because there the `define` and the
;; generated `define-syntax` share one expansion, so the getter's reference
;; carries the defining expansion's scope. `define_scoped_definition`'s doc
;; records the contract boundary this pins.
(define-syntax defpriv (syntax-rules () ((_) (define hidden-priv 10))))
(define-syntax defgetter
  (syntax-rules () ((_ g) (define-syntax g (syntax-rules () ((_) hidden-priv))))))
(defpriv)
(defgetter get)
(cond-expand (patina-vm (test-expect-fail 1)) (else))
(test-error "a generated getter cannot see a different expansion's private define" #t
  (get))

(test-end)
