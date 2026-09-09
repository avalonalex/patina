;; Hygiene — R7RS §4.3.2: a template's free identifier means what it meant
;; where the macro was *written*, and a binding a template introduces is
;; renamed throughout its scope.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (families 15,
;; 36, 37, 38 and 39; #193 Phase 1). Fourteen Rust tests became eighteen rows —
;; the seventeen below, plus one that went to `expansion/ellipsis.scm` for the
;; reason at the end of this header.
;;
;; **The other two hygiene files, and what each is for.**
;; `hygiene_matrix.rs` is a scoreboard, not a suite: 28 shapes scored against
;; chibi and Racket, read as a table when a hygiene fix moves a row. It stays
;; Rust. `hygiene.rs` is 49 tests that predate the shared helpers, and the
;; honest description is that **44 of them build a tree-walker by hand** and so
;; never run on the VM at all, while 8 use `assert_program_eval_to` and run
;; both. That is a gap, not a design — the VM is the default backend, and the
;; defects families 36 and 40 record are VM-side. Most of those 44 are ordinary
;; portable value assertions that belong here; converting them is its own job,
;; and `hygiene.rs`'s header now says so. **Add a new portable hygiene row
;; here**, where it runs on both backends and both oracles.
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   17 pass
;;   chibi                     17 pass
;;   Gauche                    17 pass
;;
;; **Nothing here diverges**, which is worth saying for a file of seventeen
;; hygiene rows. Most were live defects in Patina within the last month; three
;; were not, and the difference matters when reading them. Family 36's
;; parameter and `let` rows and family 39's row all pass *before* the fixes
;; they document — the first two because nothing had pinned the shape until PR
;; #138 broke it with the suite staying green, the third because the right
;; answer was arriving for the wrong reason. Their comments say which is which.
;; The register has no entry for this file.
;;
;; One row of family 15 is **not** here. It is the same claim as the first row
;; below with `...` as the keyword instead of `if`, and it lives in
;; `expansion/ellipsis.scm` — Gauche rejects a `syntax-rules` written where
;; `...` is bound, so keeping it here would cost Gauche this whole file. The
;; ellipsis file is already registered as one it cannot run.

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

(test-end)
