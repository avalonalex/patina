;; `let-syntax` and `letrec-syntax` — R7RS §4.3.1: a body is a body, and the
;; transformers are evaluated in the environment *outside* the form (under
;; `letrec-syntax`, inside it).
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (families 15,
;; 33 and 35, and its "What `base` found once it ran" section; #193 Phase 1).
;; Nine Rust tests became the fifteen rows below, split so that a failure names
;; the claim rather than an element of a list. Its Rust companion
;; `let_syntax.rs` keeps what a `.scm` file cannot say — that a malformed
;; binding, a non-symbol keyword or an empty body is *rejected*, which is a
;; claim about a stage and not about a value.
;;
;; The file is one file on purpose: eight of these rows were written in
;; separate Rust tests, each with its own empty top level, and Larceny's `base`
;; is what found several of them precisely because a real program has other
;; things in it. The global `f` below is that condition restored — `base`
;; defines its own `f`, which is why two rows that passed in isolation still
;; failed there.
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   15 pass
;;   chibi                     15 pass
;;   Gauche                    13 pass, 2 fail — registered as an oracle defect
;;
;; **Gauche lets a template-generated `let-syntax` capture its own sibling.**
;; The two rows it fails are the generated form; the row directly beneath them
;; is the *same shape written by hand*, and Gauche answers that one exactly as
;; we do. So it has §4.3.1's rule and loses it under expansion, which is what
;; makes this a defect claim rather than a difference of reading — see the
;; register for the evidence as recorded.

(import (scheme base) (srfi 64))

(test-begin "let-syntax")

;; ── A keyword is a binding, and binds where it stands ───────────────────────
;;
;; **Larceny family 15.** The spelling veto this replaced had no ordering, so
;; an enclosing variable of the same name won wherever one existed. Both
;; elements are one row: the first is the control that says the inner keyword
;; works at all, and reading them apart is what makes the second mean anything.
(test-equal "an inner keyword outranks an enclosing variable of the same name"
  '(1 1)
  (list (let-syntax ((f (syntax-rules () ((f x) x)))) (f 1))
        (let ((f (lambda (x) (+ x 1))))
          (let-syntax ((f (syntax-rules () ((f x) x)))) (f 1)))))

;; R7RS §4.3.1: a `let-syntax` body is a body, so its definitions are local to
;; it. `define-values` and `define-record-type` both expand to a `begin` of
;; definitions, so testing only the top level of the desugared body saw no
;; definition at all and let the names escape into the enclosing one.
;;
;; The `let-syntax` runs in a definition rather than at top level because its
;; value is not what is under test — `aa` afterwards is.
(define aa 'outer)
(define ignored-inner-aa
  (let-syntax ((noop (syntax-rules () ((_ x) x))))
    (define-values (aa bb) (values 1 2))
    (noop aa)))

(test-equal "a let-syntax body keeps definitions a macro wrapped in begin"
  'outer aa)

;; R7RS §5.3.2: a syntax definition inside a body is local to that body. Ours
;; installed itself in the enclosing environment, and then — once a body with
;; bindings got an environment of its own — only when the body's lambda
;; happened to bind nothing, which made the leak depend on the formals list.
;; Hence both shapes: one procedure with an argument, one without.
(define (with-args y)
  (define-syntax m-with-args (syntax-rules () ((_ v) (list 'withargs v))))
  (m-with-args y))

(define (without-args)
  (define-syntax m-without-args (syntax-rules () ((_ v) (list 'noargs v))))
  (m-without-args 1))

(test-equal "an internal define-syntax serves its own body"
  '((withargs 1) (noargs 1))
  (list (with-args 1) (without-args)))

(test-error "and is not visible outside it" #t (m-with-args 3))

;; ── A transformer's free identifiers ────────────────────────────────────────
;;
;; A `let-syntax` transformer's free identifier denotes the binding enclosing
;; the *form*, even where the program also defines that name at top level.
;; Larceny's `base` is what found this: the suite defines its own `f`, so the
;; assertion below still failed there after it passed in isolation — `(g 1)`
;; answered `"1"`, the suite's `number->string` wrapper, rather than 2.
;;
;; One IR below hygiene, and that is why: a template's free identifiers are
;; linked back to the macro's definition environment by *name*, for the sake of
;; a template calling a helper private to its library — and the name-only view
;; of an environment deliberately hides local variables, so it could not tell
;; this `f` from a global one. The link is asked with the macro's definition
;; scopes now, and skips a name something lexical shadows.
(define (f n) (number->string n))

(test-equal "a transformer's free reference prefers the enclosing binding"
  '(1 2)
  (let ((f (lambda (x) (+ x 1))))
    (let-syntax ((f (syntax-rules () ((f x) x)))
                 (g (syntax-rules () ((g x) (f x)))))
      (list (f 1) (g 1)))))

;; ── The three claims `base` made at once ────────────────────────────────────
;;
;; One Rust test asserting `((13 70) (1 2) (1 1))`, and three separate mistakes
;; behind it. Split, because a list of three lists says which element moved and
;; not which rule broke.
;;
;; `defs` binds `x` through a **macro**, so reading the body's source forms saw
;; no definition and let it escape: `x` stayed 13 and `y` became 70 only if the
;; `def` really did define a *local* `x` of 56.
(define (defs)
  (let ((x 13))
    (define y 14)
    (let-syntax ((def (syntax-rules () ((_ var val) (define var val)))))
      (def x 56)
      (set! y (+ x y)))
    (list x y)))

(test-equal "a definition a macro makes in a let-syntax body stays in it"
  '(13 70) (defs))

;; Under `let-syntax` the transformers are evaluated outside the form, so `g`'s
;; `f` is the enclosing *variable* and `(g 1)` is 2. A keyword bound unscoped
;; could never outrank one, so this used to answer with the outer variable in
;; both positions.
(define (scope)
  (let ((f (lambda (x) (+ x 1))))
    (let-syntax ((f (syntax-rules () ((f x) x)))
                 (g (syntax-rules () ((g x) (f x)))))
      (list (f 1) (g 1)))))

(test-equal "a let-syntax transformer does not see its siblings" '(1 2) (scope))

;; The same body under `letrec-syntax`, where the sibling *is* in scope, so
;; `(g 1)` reaches the keyword and answers 1. The pair is the point: a
;; implementation that gets `let-syntax` wrong in this direction passes the row
;; above by accident, and only differing here shows the two forms are distinct.
(define (rec-scope)
  (let ((f (lambda (x) (+ x 1))))
    (letrec-syntax ((f (syntax-rules () ((f x) x)))
                    (g (syntax-rules () ((g x) (f x)))))
      (list (f 1) (g 1)))))

(test-equal "a letrec-syntax transformer does" '(1 1) (rec-scope))

;; ── The same forms, generated by a template ─────────────────────────────────
;;
;; **Larceny family 35**, the review round of the family-33 fix. A `let-syntax`
;; puts its scope on its body *as written* and — for `letrec-syntax` — on its
;; transformers, which is what lets an introduced binder be bound at its own
;; scopes plus that one.
;;
;; `gen6`'s keyword is referenced from inside its own transformer, which the
;; unscoped binding used to satisfy by name; `gen7`'s from a transformer in the
;; body. Both were unbound after the first fix.
(define-syntax gen6
  (syntax-rules ()
    ((_ name) (letrec-syntax ((name (syntax-rules () ((_) 'done6) ((_ x . r) (name . r)))))
                (name 1)))))

(define-syntax gen7
  (syntax-rules ()
    ((_ name) (let-syntax ((name (syntax-rules () ((_) 'v))))
                (let-syntax ((g (syntax-rules () ((_) (name))))) (g))))))

(test-equal "a generated keyword is reachable from its own transformer"
  'done6 (gen6 foo))

(test-equal "and from a transformer in its body" 'v (gen7 bar))

;; Under `let-syntax` a transformer does not see its siblings, and that holds
;; when the whole form came out of a template: `a`'s `(b)` is the outer `b`.
;; The first fix bound the generated keyword at the template's scopes alone,
;; which every reference from that expansion carries — the sibling's
;; transformer included.
;;
;; **This is the row Gauche fails**, answering `sibling-b`; chibi answers as we
;; do. See the header and the register.
(define-syntax b (syntax-rules () ((_) 'outer-b)))

(define-syntax gen-siblings
  (syntax-rules ()
    ((_) (let-syntax ((a (syntax-rules () ((_) (b))))
                      (b (syntax-rules () ((_) 'sibling-b))))
           (a)))))

(test-equal "a generated let-syntax keeps siblings out of its transformers"
  'outer-b (gen-siblings))

;; The same, with the outer keyword itself bound by a `let-syntax` rather than
;; at top level — the direction where the outer binding is not simply "the
;; global one". Gauche fails this too, answering `sibling-b2`.
(test-equal "and the same where the outer keyword is itself a let-syntax one"
  'outer-b2
  (let-syntax ((b2 (syntax-rules () ((_) 'outer-b2))))
    (let-syntax ((gen2 (syntax-rules ()
                         ((_) (let-syntax ((a (syntax-rules () ((_) (b2))))
                                           (b2 (syntax-rules () ((_) 'sibling-b2))))
                                (a))))))
      (gen2))))

;; The control for the two rows above, and the reason the register calls
;; Gauche's answer a defect rather than a reading: the identical form, written
;; in source instead of generated. All three implementations answer `outer-b`
;; here — so §4.3.1's rule is present in all three and one of them loses it
;; under expansion.
(test-equal "the same shape written by hand, which all three get right"
  'outer-b
  (let-syntax ((a (syntax-rules () ((_) (b))))
               (b (syntax-rules () ((_) 'sibling-b))))
    (a)))

;; A user's `(k)` passed into a template that wraps it in a `let-syntax`
;; binding `k` means the user's `k`: the generated keyword's binding carries
;; the template's scope, which the user's reference never does. Pre-existing —
;; the keyword used to be bound at the body's scopes alone, which every symbol
;; in the body resolves with.
(define (k) 'users-k)

(define-syntax gen-around
  (syntax-rules ()
    ((_ body) (let-syntax ((k (syntax-rules () ((_) 'captured)))) body))))

(test-equal "a user's symbol is not captured by a generated keyword"
  'users-k (gen-around (k)))

;; **Larceny family 33.** `mq`'s template writes `(quote d)`; a `let-syntax`
;; binding `quote` around the *call* has nothing to do with it. Patina used to
;; bind a `let-syntax` keyword unscoped as well as scoped, which made it
;; reachable from every reference of that spelling — including one another
;; macro introduced — and since the captured expansion introduces `quote`
;; again, the capture repeated until the stack went.
;;
;; The import half of that family, which needs a library to have anything
;; private to reach, is not here; it cannot be arbitrated by chibi, which does
;; not support `define-library` in a script.
(define-syntax mq (syntax-rules () ((mq d) (quote d))))

(test-equal "a template's quote is not captured by a use-site let-syntax"
  'hello
  (let-syntax ((quote (syntax-rules () ((_ x) 'captured))))
    (mq hello)))

(test-end)
