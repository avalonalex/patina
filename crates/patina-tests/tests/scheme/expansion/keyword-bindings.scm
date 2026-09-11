;; Syntactic keywords are bindings, not spellings — the half of that rule a
;; portable program can observe from inside.
;;
;; Migrated from `crates/patina-tests/tests/syntax_as_a_value.rs` and
;; `core_syntax_bindings.rs` (#193 Phase 2). Both files stay, smaller, and say
;; why: what remains there is either a *rejection* the desugarer makes before
;; the program runs (no `guard` in a program can observe its own form being
;; refused), a keyword rebound at the top level (which would rebind it for
;; every row after it in a shared file), or a `define-library` (which chibi
;; cannot define in a script). What is here is what they proved *works* —
;; the false positives a binding-based keyword rule could easily have had —
;; and it is exactly what an oracle can arbitrate.
;;
;; Design and staging: `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md`.
;;
;; Divergences are recorded in `DIVERGENCES.tsv`, not restated here.

(import (scheme base) (rename (scheme base) (begin blk) (define-syntax defmac))
        (srfi 64))

(test-begin "keyword-bindings")

;; ── A local binding shadows a keyword ──────────────────────────────────────
;;
;; Local bindings are not in the desugarer's environment at all, so they are
;; handled by `shadowed_names` rather than by the lookup — the part of the
;; rule most likely to be forgotten.

(test-equal "a let-bound keyword is an ordinary variable" '(5 7 3)
  (list (let ((else 5)) else)
        ((lambda (if) if) 7)
        (let ((cond 1) (quote 2)) (+ cond quote))))

(test-equal "a let-bound keyword in head position is a call" 'shadowed
  (let ((if (lambda (a b c) 'shadowed))) (if 1 2 3)))

(define (call-begin-with begin) (begin 1 2))
(test-equal "a formal named like a keyword is the formal" '(1 2)
  (call-begin-with list))

;; A use site that rebinds `else` stops it matching as `cond`'s literal —
;; R7RS matches `syntax-rules` literals by binding, not by spelling, and this
;; is the case that tells the two apart.
(test-equal "a rebound else does not match" 2
  (let ((else #f)) (cond (else 1) (#t 2))))

;; ── An internal definition shadows a keyword over the whole body ───────────
;;
;; Like `letrec*` — over the whole body, including the forms before it
;; (R7RS §5.3.2). The body scan has to run before any form is desugared, or
;; the value-position check sees the core binding and rejects a legal program
;; (audit C2), and head position silently picks the core form over the local
;; binding (audit D6).

;; C2: value position. Rejected outright before the scan existed.
(test-equal "an internal define over a keyword, in value position" '(4 4 4)
  (list (let () (define if 3) (+ if 1))
        (let () (define when 3) (+ when 1))
        ((lambda () (define if 3) (+ if 1)))))

;; D6: head position. The pre-existing half — silently answered 2, because
;; core `if` claimed the head over the body's own binding.
(test-equal "an internal define over a keyword, in head position"
  '(shadowed shadowed-cond)
  (list (let () (define (g) (define if (lambda (a b c) 'shadowed)) (if 1 2 3)) (g))
        (let () (define cond (lambda args 'shadowed-cond)) (cond 1 2))))

;; `begin` splices into the body it appears in, so its definitions bind there
;; too; an inner body's definitions stay inside it; and a body that defines
;; nothing of the sort is untouched.
(test-equal "the body scan: begin splices, inner bodies stay inner" '(5 7 yes)
  (list ((lambda () (begin (define if 4)) (+ if 1)))
        ((lambda () (define (g) (define if 7) if) (g)))
        ((lambda () (define x 3) (if (> x 1) 'yes 'no)))))

;; ── define-syntax is recognized through its binding ────────────────────────
;;
;; F4: body-position `define-syntax` was the last keyword still recognized by
;; spelling, so a body that binds the *name* still had its call read as a
;; macro definition.

;; Shadowed by a formal, `(define-syntax foo 2)` is an ordinary call of the
;; procedure the formal is bound to. By spelling it was read as a macro
;; definition named `foo` with 2 for a transformer.
(define foo 10)
(test-equal "a formal named define-syntax is called" 12
  ((lambda (define-syntax) (define-syntax foo 2)) (lambda (a b) (+ a b))))

;; Reached under an import rename, it still defines a macro.
(test-equal "define-syntax renamed on import still defines a macro" 12
  ((lambda () (defmac m (syntax-rules () ((_ x) (* x 3)))) (m 4))))

;; ── Keywords travel through import sets ────────────────────────────────────

;; The keyword survives being renamed on the way in and reaching the use site
;; under the new name. The VM once loaded the library and left `blk` unbound
;; while the tree-walker rejected it; chibi and Gauche made it work.
(test-equal "a keyword renamed on import works under its new name" 2
  (blk 1 2))

;; ── Syntax that is not in value position ───────────────────────────────────

;; Quoted data is data: it desugars to a literal and never reaches the check.
(test-equal "quoted keywords are data" '((if cond else) if (a 2 else))
  (list '(if cond else) (car '(if)) `(a ,(+ 1 1) else)))

;; The derived forms are macros whose expansions the check sees. Any one of
;; them emitting a keyword in value position would fail here — the cheapest
;; guard against the rule being subtly too strict. A smoke test, deliberately:
;; each form has its own suite.
(define-record-type <p> (mk a) p? (a p-a))
(test-equal "the derived forms still expand" '(done 3 caught 7 done)
  (list (do ((i 0 (+ i 1))) ((= i 2) 'done))
        (let-values (((a b) (values 1 2))) (+ a b))
        (guard (e (#t 'caught)) (raise 1))
        (p-a (mk 7))
        (let loop ((i 0)) (if (= i 2) 'done (loop (+ i 1))))))

;; `apply` is a real procedure binding, not a keyword or a macro, so it stays
;; a value — although it is the one head symbol the desugarer still
;; recognizes by spelling.
(test-assert "apply is still a value" (procedure? apply))

;; The refusal to `set!` a keyword (in `syntax_as_a_value.rs`) is about what
;; the name denotes, not about `set!`.
(test-equal "set! on an ordinary variable is untouched" 2
  (let ((x 1)) (set! x 2) x))

(test-end)
