;; `define-values` — R7RS §5.3.3.
;;
;; Migrated whole from `crates/patina-tests/tests/define_values.rs` (#193
;; Phase 1). Every row was one `assert_program_eval_to`; 12 rows in, 12 out,
;; plus one added below.
;;
;; The macro lives in `lib/scheme/base/binding.scm`, and the shapes here are its
;; formals grammar: the empty list, one name, several names, a dotted tail, and
;; a bare symbol standing for the whole list.
;;
;; **Every migrated row keeps its `(let () …)` wrapper**, which is not
;; incidental. R7RS §5.3.3 makes `define-values` a *definition*, so these are
;; internal definitions in a body — `letrec*` scope, a local slot — and hoisting
;; them to this file's top level would move them to a different resolution path.
;; The one row that *does* test the top level says so.

(import (scheme base) (srfi 64))

(test-begin "define-values")

;; ── The formals grammar ─────────────────────────────────────────────────────

(test-equal "no variables, evaluated for effect" 'ok
  (let ()
    (define-values () (values))
    'ok))

;; Discarding values is not the same as producing none: the body still runs and
;; its values are dropped.
(test-equal "no variables, and the expression still produces some" 42
  (let ()
    (define-values () (values 1 2 3))
    42))

(test-equal "one variable" 1
  (let ()
    (define-values (x) (values 1))
    x))

;; A single value need not come from `values` at all.
(test-equal "one variable, bound to a plain expression" 42
  (let ()
    (define-values (x) 42)
    x))

(test-equal "two variables" 3
  (let ()
    (define-values (x y) (values 1 2))
    (+ x y)))

(test-equal "three variables" 6
  (let ()
    (define-values (x y z) (values 1 2 3))
    (+ x y z)))

;; Four is past the point where a `syntax-rules` ellipsis has to be doing the
;; work rather than a hand-written clause.
(test-equal "four variables, so the ellipsis is what expands" '(1 2 3 4)
  (let ()
    (define-values (a b c d) (values 1 2 3 4))
    (list a b c d)))

;; ── Dotted and symbol formals ───────────────────────────────────────────────

;; A bare symbol takes every value as a list, the way `(lambda args …)` does.
(test-equal "a symbol for the whole list of values" 3
  (let ()
    (define-values x (values 1 2))
    (apply + x)))

(test-equal "a dotted tail collects the rest" 10
  (let ()
    (define-values (x y . z) (values 1 2 3 4))
    (+ x y (car z) (cadr z))))

;; ── Each name gets its own value ────────────────────────────────────────────
;;
;; The rows above sum, which a macro that bound every name to the same value
;; would still satisfy for symmetric operands. These read the names separately.

(test-equal "two names bind separately" '(10 20)
  (let ()
    (define-values (x y) (values 10 20))
    (list x y)))

(test-equal "three names bind separately" '(10 20 30)
  (let ()
    (define-values (x y z) (values 10 20 30))
    (list x y z)))

(test-equal "the operands are evaluated, not just matched" 15
  (let ()
    (define-values (x y) (values (+ 1 2) (* 3 4)))
    (+ x y)))

;; ── At the top level ────────────────────────────────────────────────────────

;; R7RS §5.3.3 puts `define-values` wherever a definition may appear, which
;; includes the top level — a different resolution path from every row above,
;; and one no row in the `.rs` file covered. Added here rather than left as a
;; gap; portable, and verified on chibi and Gauche.
(define-values (top-a top-b . top-rest) (values 'a 'b 'c 'd))
(test-equal "define-values binds at the top level too" '(a b (c d))
  (list top-a top-b top-rest))

;; `(x . y)` — a dotted formal with **no** fixed names, which is a different
;; expansion from every row above: `binding.scm`'s `var1 ...` matches nothing,
;; so the intermediate `set-cdr!` step is elided and `var-dot` reads `(cdr var0)`
;; directly. Every other dotted row here drives `var1 ...` with one repetition.
;;
;; At the top level rather than in a `(let () …)` because Gauche rejects this
;; shape as an *internal* definition — "proper list required for function
;; application or macro use: (x . y)" — while accepting it at the top level, as
;; do chibi and both backends. Measured 2026-09-07.
(define-values (dotted-head . dotted-rest) (values 1 2 3))
(test-equal "a dotted formal with no fixed names" '(1 (2 3))
  (list dotted-head dotted-rest))

;; Both top-level forms above run outside any assertion, which is the trade for
;; testing the top-level path at all: if that expansion breaks, the file is
;; reported as "failed to run" rather than as one bad row. They are last on
;; purpose — anything appended below inherits that.

(test-end)
