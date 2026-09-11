;; What a template introduces across recursive expansions — R7RS §4.3.2's
;; renaming rule, applied to the parameters and definitions a macro makes, and
;; the counterpart it must not break: a macro the same expansion generates
;; still reaches those definitions.
;;
;; Migrated from `crates/patina-tests/tests/compliance/macros_advanced.rs`
;; (#193), its "Template-introduced identifiers across recursive expansions"
;; section, which is deleted with the rest of that module. The module's other
;; rows are in `syntax-rules.scm`, which lists where each part went.
;;
;; The shape throughout: a recursive macro introduces the *same* template
;; identifier — `a`, `tmp` — once per expansion. Each carries its own
;; expansion's scope, so each is a distinct binding. Rejecting them as
;; duplicate parameters broke SRFI 156's `is`/`isnt`; collapsing them into one
;; definition broke SRFI 165's `define-computation-type`, which is why
;; `(srfi 166)` could not load.
;;
;; **Every row here uses its own spellings**, for introduced names as well as
;; the caller's. Each Rust test was a program of its own; in one file, two rows
;; introducing the same name at top level would meet in the VM's by-name
;; relinking — the jabberwocky-steal defect `hygiene.scm`'s family-40 section
;; describes — and a row could pass or fail on another row's definitions. The
;; exception is deliberate: the last two rows introduce `tmp`, because the
;; names they are about are spelled from it — the minted `tmp__#N`, and a
;; global `tmp` the program writes.
;;
;; `hygiene.scm`'s family-40 rows are the other direction of the same
;; contract: one expansion's introduced definition must *not* be reachable
;; from a different expansion's template. Read both before changing how an
;; introduced definition is named or relinked.
;;
;; ── Measured 2026-09-11 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   17 pass, 1 expected failure — pinned below
;;   chibi                     18 pass
;;   Gauche                    16 pass, 2 fail — registered as latitude
;;
;; The expected failure is a Patina defect this migration found: a body's
;; introduced definition is reachable from the use site by name. Gauche's two
;; are the duplicate-formal rows, which R7RS leaves it free to accept.

(import (scheme base) (scheme eval) (srfi 64))

(test-begin "introduced-definitions")

;; ── Parameters ──────────────────────────────────────────────────────────────

;; The accumulator spells every parameter `a`, but each is introduced by a
;; different expansion. Gauche and Chez Scheme both answer (10 20), per the
;; Rust original's comment.
(define-syntax gen-params
  (syntax-rules ()
    ((_ () (args ...)) (lambda (args ...) (list args ...)))
    ((_ (x . rest) (args ...)) (gen-params rest (args ... a)))))

(test-equal "a recursive macro introduces distinct parameters" '(10 20)
  ((gen-params (1 2) ()) 10 20))

;; The same rule for the rest parameter of an improper formals list.
(define-syntax gen-rest-params
  (syntax-rules ()
    ((_ () (args ...)) (lambda (args ... . r) (list args ... r)))
    ((_ (x . rest) (args ...)) (gen-rest-params rest (args ... a)))))

(test-equal "and a distinct rest parameter" '(10 20 (30 40))
  ((gen-rest-params (1 2) ()) 10 20 30 40))

;; Hand-written duplicates share a scope set, so they are still an error. R7RS
;; §4.1.4 makes a repeated formal "an error" without requiring a signal, so
;; the refusal is Patina's choice, and chibi's; Gauche 0.9.15 accepts both
;; lambdas (registered as latitude). Evaluated with `eval` so the refusal is a
;; caught error rather than a file that will not compile, and the first row is
;; the control that says the `eval` itself works.
(test-equal "distinct hand-written formals are fine" 1
  ((eval '(lambda (q r) q) (environment '(scheme base))) 1 2))
(test-error "a hand-written duplicate formal is still an error" #t
  (eval '(lambda (q q) q) (environment '(scheme base))))
(test-error "and a rest parameter duplicating a formal" #t
  (eval '(lambda (q . q) q) (environment '(scheme base))))

;; SRFI 156's reference implementation, reduced to the shape that failed:
;; each `_` placeholder becomes a fresh lambda parameter named `arg`.
(define-syntax infix/postfix
  (syntax-rules ()
    ((infix/postfix x somewhat?) (somewhat? x))
    ((infix/postfix left related-to? right) (related-to? left right))))

(define-syntax extract-placeholders
  (syntax-rules (_)
    ((extract-placeholders final () () body)
     (final (infix/postfix . body)))
    ((extract-placeholders final () args body)
     (lambda args (final (infix/postfix . body))))
    ((extract-placeholders final (_ op . rest) (args ...) (body ...))
     (extract-placeholders final rest (args ... arg) (body ... arg op)))
    ((extract-placeholders final (arg op . rest) args (body ...))
     (extract-placeholders final rest args (body ... arg op)))
    ((extract-placeholders final (_) (args ...) (body ...))
     (extract-placeholders final () (args ... arg) (body ... arg)))
    ((extract-placeholders final (arg) args (body ...))
     (extract-placeholders final () args (body ... arg)))))

(define-syntax identity-syntax
  (syntax-rules () ((identity-syntax form) form)))

(define-syntax is
  (syntax-rules ()
    ((is . something)
     (extract-placeholders identity-syntax something () ()))))

(test-equal "SRFI 156's placeholder shape" '(#t #f)
  (list ((is _ < _) 1 2) ((is _ < _) 2 1)))

;; ── Definitions ─────────────────────────────────────────────────────────────

;; The same rule for a *definition*, which is where it was still broken:
;; `CoreExprKind::Define` carried no scopes, so every expansion of one
;; template defined the same variable and the last value won. `btmp` is
;; introduced once per element, so the three definitions are three bindings.
;; Patina answered (3 3 3); chibi and Gauche give (1 2 3).
(define-syntax bind-each
  (syntax-rules ()
    ((_ () ((name val btmp) ...))
     (begin (define btmp val) ...
            (define name btmp) ...))
    ((_ ((n v) . rest) (acc ...))
     (bind-each rest (acc ... (n v btmp))))))

(bind-each ((top-a 1) (top-b 2) (top-c 3)) ())

(test-equal "a recursive macro introduces distinct definitions" '(1 2 3)
  (list top-a top-b top-c))

;; The same inside a body rather than at top level — the two travel different
;; paths through the VM's alpha-renaming pass.
(define (bind-in-a-body)
  (bind-each ((body-a 1) (body-b 2) (body-c 3)) ())
  (list body-a body-b body-c))

(test-equal "and distinct definitions in a body" '(1 2 3) (bind-in-a-body))

;; The definitions may arrive nested inside another macro's `begin` —
;; `define-values` expands to one, so a macro using it produces two levels. A
;; one-level scan for definitions found the outer group and none of its
;; members, which left the collapse in place on the VM only.
(define-syntax bind-each-values
  (syntax-rules ()
    ((_ () ((name val vtmp) ...))
     (begin (define-values (vtmp ...) (values val ...))
            (define name vtmp) ...))
    ((_ ((n v) . rest) (acc ...))
     (bind-each-values rest (acc ... (n v vtmp))))))

(bind-each-values ((dv-a 1) (dv-b 2) (dv-c 3)) ())

(test-equal "and through define-values" '(1 2 3) (list dv-a dv-b dv-c))

;; A definition nested two `begin` levels deep in a body is still that body's
;; definition, so each call gets its own. `define-values` expands to a `begin`
;; of definitions, which the macro wraps in another — and the VM's two passes
;; disagreed about how deep to look, so `alpha_rename` renamed it to a local
;; that `pass1_analysis` never gave a slot, leaving a global shared by every
;; call. The reads are sequenced: each closure's state is the claim.
(define-syntax define-two
  (syntax-rules ()
    ((_ a b) (begin (define-values (a b) (values 1 2))))))

(define (make-counter) (define-two p q) (lambda () (set! p (+ p 10)) p))

(test-equal "a definition two begins deep is local to its body" '(11 11 21)
  (let* ((c1 (make-counter))
         (c2 (make-counter))
         (r1 (c1))
         (r2 (c2))
         (r3 (c1)))
    (list r1 r2 r3)))

;; ── A generated macro still reaches the expansion's definitions ─────────────
;;
;; The counterpart the fix must not break: a macro-introduced definition stays
;; reachable from a macro that the *same* expansion generated, even though
;; that reference is resolved by name rather than by scopes
;; (`link_definition_env_refs`). This is the R7RS suite's `jabberwocky` shape,
;; and making the definition scope-only broke it.
(define-syntax jabberwocky
  (syntax-rules ()
    ((_ hatter)
     (begin
       (define march-hare 42)
       (define-syntax hatter
         (syntax-rules ()
           ((_) march-hare)))))))

(jabberwocky mad-hatter)

(test-equal "a generated macro reaches an introduced definition" 42
  (mad-hatter))

;; ...and it reaches the *binding*, not a copy of its value. Giving the
;; definition a second, name-only cell made the two disagree the moment the
;; expansion mutated one of them. chibi and Gauche answer 2, per the Rust
;; original's comment.
(define-syntax jab-mutate
  (syntax-rules ()
    ((_ h b) (begin (define mutated-hare 1)
                    (define (b) (set! mutated-hare 2))
                    (define-syntax h (syntax-rules () ((_) mutated-hare)))))))

(jab-mutate get-mutated bump-mutated)

(test-equal "and sees a mutation of it" 2
  (begin (bump-mutated) (get-mutated)))

;; ...and it can *write* it. The `set!` sits in a macro the expansion
;; generated, so it arrives relinked to the bare name rather than carrying
;; scopes — a different path from the row above, whose `set!` is in the outer
;; template. Giving reads a name-only view without giving writes the same one
;; left the name readable and unassignable. chibi and Gauche answer 5.
(define-syntax jab-assign
  (syntax-rules ()
    ((_ h s) (begin (define assigned-hare 1)
                    (define-syntax h (syntax-rules () ((_) assigned-hare)))
                    (define-syntax s (syntax-rules () ((_ v) (set! assigned-hare v))))))))

(jab-assign get-assigned put-assigned)

(test-equal "and can assign to it" 5
  (begin (put-assigned 5) (get-assigned)))

;; The same reach, inside a body.
(define-syntax def-and-use
  (syntax-rules ()
    ((_ getter)
     (begin (define secret 42)
            (define-syntax getter (syntax-rules () ((_) secret)))))))

(define (use-in-a-body)
  (def-and-use get-secret)
  (get-secret))

(test-equal "and reaches it in a body" 42 (use-in-a-body))

;; ── The use site does not reach them ────────────────────────────────────────
;;
;; The rows above are reach from *inside* the expansion. From outside it, an
;; introduced definition is not the caller's: R7RS §4.3.2 renames a binding a
;; template inserts "throughout its scope", so the caller's identifier of the
;; same spelling is a different one. At top level §4.3.2 lets "a global
;; variable definition" off ("may or may not introduce a binding"), which is
;; why only a body is asserted here.
;;
;; `compliance/macros_advanced.rs`'s `test_nested_macro_with_ellipsis_escape`,
;; as it was written: the template names the macro it generates, and the body
;; then calls it by that name. The Rust test asserted Patina's answer, 1.
;; chibi 0.12 and Gauche 0.9.15 both refuse — "undefined variable", "unbound
;; variable: apply-to-list" — and so does R7RS, so the row asserts the refusal.
;; The ellipsis half of the test, with the caller supplying the name, is in
;; `ellipsis.scm`, where all four agree.
;;
;; Patina defect, #269: a definition a template introduces in a body —
;; `define` or `define-syntax` — is reachable from the use site by its bare
;; name, on both backends. The same holds at top level, which R7RS permits.
(cond-expand (patina (test-expect-fail 1)) (else))
(test-error "a keyword a template introduces in a body is not the caller's" #t
  (let ()
    (define-syntax listify
      (syntax-rules ()
        ((listify e)
         (define-syntax apply-to-list
           (syntax-rules ()
             ((apply-to-list arg (... ...))
              (e (list arg (... ...)))))))))
    (listify car)
    (apply-to-list 1 2 3)))

;; ── Top-level names the VM mints ────────────────────────────────────────────

;; Two *separate* top-level forms expanding the same macro must not share the
;; definitions it introduces. The VM renames a macro-introduced top-level
;; definition to a global no source code mentions, and that name used to come
;; from a counter `alpha_rename` resets per form — so the second form minted
;; the same name and silently took the first's binding. The name is derived
;; from the definition's scope set now.
(define-syntax mk-pairs
  (syntax-rules ()
    ((_ () ((name val ptmp) ...))
     (begin (define ptmp val) ... (define (name) ptmp) ...))
    ((_ ((n v) . rest) (acc ...)) (mk-pairs rest (acc ... (n v ptmp))))))

(mk-pairs ((form1-a 1) (form1-b 2)) ())
(mk-pairs ((form2-c 3) (form2-d 4)) ())

(test-equal "two forms expanding one macro get separate definitions" '(1 2 3 4)
  (list (form1-a) (form1-b) (form2-c) (form2-d)))

;; The minted name must not be one a program can write. These names outlive
;; the form that made them, so a collision is a silent wrong value. The
;; spelling carries a space, which the reader cannot produce outside `|…|`, so
;; the `__#N` form the counter used to mint reaches nothing. The Rust original
;; wrote those names bare; `#` is not an identifier character in R7RS, so they
;; are written with vertical bars here, which is the same symbol.
(define-syntax mk-minted
  (syntax-rules ()
    ((_ () ((name val tmp) ...))
     (begin (define tmp val) ... (define (name) tmp) ...))
    ((_ ((n v) . rest) (acc ...)) (mk-minted rest (acc ... (n v tmp))))))

(mk-minted ((minted-a 41) (minted-b 2)) ())
(define |tmp__#0| 99)
(define |tmp__#1| 99)

(test-equal "a minted global name is not writable from source" '(41 2)
  (list (minted-a) (minted-b)))

;; A macro's introduced definition must not overwrite a global of the same
;; name that source code wrote. The bare name is answered by an `Environment`
;; alias rather than a definition, and `get` consults aliases only after real
;; bindings — so the user's `tmp` wins here. Last in the file because it is
;; the one row whose global shares a spelling with an introduced name.
(define tmp 5)
(mk-minted ((clobber-a 1) (clobber-b 2)) ())

(test-equal "an introduced definition does not clobber a user's global" '(5 1 2)
  (list tmp (clobber-a) (clobber-b)))

(test-end)
