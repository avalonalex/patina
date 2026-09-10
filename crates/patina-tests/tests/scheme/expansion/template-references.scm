;; A template's free identifiers mean what they meant where the macro was
;; *written* — R7RS §4.3.2 referential transparency — including when what they
;; meant is private to the library that wrote them.
;;
;; **Moved from `crates/patina-tests/tests/larceny_families.rs`** (families 33
;; and 35, #193 Phase 1). Six rows, and the last of the Larceny macro block
;; that a suite file can hold: what remains there is family 40, which is a
;; genuine backend divergence.
;;
;; ── Why this file needs libraries, and what that costs ──────────────────────
;;
;; Every row here turns on a template reaching something the *program* cannot
;; see, or on the program meaning something different by a name the template
;; also uses. Neither can be staged without a second library: a `let-syntax` in
;; the same program shares the program's bindings, which is the very thing
;; being distinguished. So the file defines five `(probe …)` libraries inline.
;;
;; **chibi 0.12 cannot run this file at all.** It does not support
;; `define-library` in a script — measured 2026-09-09, the body's `define`
;; arrives at the top level and it reports "unexpected define", or the library
;; is simply never registered. Whether the report requires it to is not settled
;; here: R7RS describes how a library is written and leaves how one is *located*
;; to the implementation, and chibi loads libraries from its load path. That is
;; a reading, not a citation — nobody has checked the text against it. That is registered as `*` / `incomplete` in
;; `DIVERGENCES.tsv` rather than worked around, so the lane holds the claim and
;; reports it if chibi gains the support. Gauche runs the file and arbitrates
;; five of the six rows.
;;
;; **The import set is the other half of the staging.** `(scheme base)`'s
;; `quote`, `car`, `cons`, `list` and `list?` are excluded and SRFI 101 supplies
;; those five names instead — its `list` builds a random-access list, which is
;; not a pair — while `(prefix (scheme base) r7:)` keeps the ordinary ones
;; reachable. So a template's `list` reaching the *library's* meaning is
;; observable as a pair, and the program's as something else. Expected values
;; are therefore built with `r7:list` and friends: an ordinary `'(1 2)` in this
;; file is a random-access list, not a pair, which is what
;; `expansion/quasiquote.scm` records learning the hard way.
;;
;; **The `except` list is long, and only five of its names carry the test** —
;; `quote`, `car`, `cons`, `list`, `list?`. The other fifteen are there because
;; SRFI 101 exports them too, and importing two bindings of one identifier is
;; an error: excluding only the five leaves `pair?`, `map`, `append`, `cdr` and
;; eleven more ambiguous, in the one file where "which `pair?` is this" is the
;; subject. Measured 2026-09-09: chibi warns ("importing already defined
;; binding: quote") and lets the later import win, Patina and Gauche say
;; nothing at all, and with the full list chibi stops warning.
;;
;; `(only (srfi 101) quote car cons list list?)` would say this far better and
;; **cannot be used while the lane runs the 0.12.0 release**: there, `only`
;; resolves against a library's *internal* names, so `(only (srfi 101) car)`
;; fails with "importing unknown binding: car" even though the export alist it
;; prints contains `(car . ra:car)`. `except` is unaffected.
;;
;; **Already fixed upstream, so nothing was filed.** Checked 2026-09-09 against
;; chibi master at `bb9b3215`, built from source: the same program answers `1`.
;; The fix is `0cf331e5`, "Allow import only on renamed identifiers." Switch
;; this import to the `only` form when the lane's chibi carries that commit —
;; it says in five names what the `except` list says in twenty.
;;
;; **What proves the staging took effect** is the relinking row's second
;; element, `(r7:pair? (r7:cadr v))` answering `#f`: that can only happen if
;; the `(list 1 2)` the program wrote really did build a random-access list.
;; Without a control like it the file would pass unchanged on an implementation
;; where the import did nothing — the first row, for instance, answers the same
;; either way. `expansion/quasiquote.scm` names its own control for the same
;; reason, and the two files share this apparatus; a correction to one belongs
;; in both.
;;
;; ── Measured 2026-09-09 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   6 pass
;;   Gauche                    5 pass, 1 skip
;;   chibi                     does not complete — registered
;;
;; ── One row was rewritten, and the reason is worth reading ──────────────────
;;
;; The ellipsis-escape row's `mk` used to introduce its *own* `g`:
;; `(begin (helper 0) (define-syntax g …))`, called as `(mk)`, with `(g)`
;; afterwards at top level. That works on the VM and nowhere else — Gauche says
;; "unbound variable: g", and it is precisely the leniency **Larceny family 40**
;; pins as a VM-only divergence that chibi and the tree-walker both refuse: one
;; expansion's private definition is not another expansion's to see. The row is
;; about what a reference under `(... …)` resolves to, not about that, so `mk`
;; takes the name from its caller now. The row says what it means to say, and
;; an oracle can arbitrate it.

(define-library (probe lit)
  (import (scheme base))
  (export lit)
  (begin (define-syntax lit (syntax-rules () ((_) '(1 2))))))

(define-library (probe both)
  (import (scheme base))
  (export both)
  (begin (define-syntax both (syntax-rules () ((_ e) (list 'template e))))))

(define-library (probe qq)
  (import (scheme base))
  (export qq qq2 qq3)
  (begin
    (define (helper x) (* 6 x))
    (define-syntax qq (syntax-rules () ((_ e) `(a (quote b) ,e))))
    (define-syntax qq2 (syntax-rules () ((_ e) `(a '(b ,(helper 7)) ,e))))
    (define-syntax qq3 (syntax-rules () ((_ e) `#(a 'b ,e))))))

(define-library (probe wq)
  (import (scheme base))
  (export outer)
  (begin
    (define-syntax with-q (syntax-rules () ((_ q) (q (list 1)))))
    (define-syntax outer (syntax-rules () ((_) (with-q quote))))))

(define-library (probe esc)
  (import (scheme base))
  (export mk def-tagger)
  (begin
    (define (helper x) (* 5 x))
    (define (tag . xs) (cons 'tagged xs))
    (define-syntax mk
      (syntax-rules ()
        ((_ name) (begin (helper 0)
                         (define-syntax name (syntax-rules () ((_) (... (helper 1)))))))))
    (define-syntax def-tagger
      (syntax-rules ()
        ((_ name) (define-syntax name (... (syntax-rules () ((_ x ...) (tag x ...))))))))))

(import (scheme eval) (scheme repl)
        (except (scheme base)
          quote car cdr caar cadr cdar cddr cons pair? null?
          list list? make-list length append reverse
          list-tail list-ref map for-each)
        (prefix (scheme base) r7:)
        (srfi 101)
        (srfi 64)
        (probe lit) (probe both) (probe qq) (probe wq) (probe esc))

(test-begin "template-references")

;; ── A template's `quote` is the definition site's ───────────────────────────
;;
;; **Larceny family 33**, the import half. SRFI 101 exports its own `quote`,
;; which builds random-access lists. A *literal* `'(1 2)` in another library's
;; template must be a pair — that library was written against `(scheme base)`'s
;; `quote` — while the user's own `'(1 2)` here is SRFI 101's. Fixed
;; 2026-08-26: the relinker rewrites a `quote` head it used to skip, and the
;; template compiler compiles the `quote` of a literal datum as a *reference*
;; rather than emitting it verbatim with the datum.
;;
;; Three claims, and the first is about SRFI 101's `quote`, not the standard's
;; — every `'…` in this file goes through it: a quoted datum is the same object
;; each time the procedure runs, so `ra:quote` is not rebuilding the list per
;; call. Then: the program's `car` is SRFI 101's and reads a random-access
;; list; and `(lit)`, whose template holds the literal, is a pair, because the
;; library it was written in means `(scheme base)`'s `quote` by it.
(test-equal "a template's quote is the definition site's under an imported one"
  (r7:list #t 1 #t)
  (r7:list (let ((f (lambda () '(x)))) (r7:eq? (f) (f)))
           (car '(1 2))
           (r7:pair? (lit))))

;; ── The relinker rewrites the template, not the user's argument ─────────────
;;
;; **Larceny family 35.** `both`'s template uses `(scheme base)`'s `list`; the
;; program's `list` is SRFI 101's. The template's reference is relinked to the
;; definition site's — that is referential transparency — but the `(list 1 2)`
;; the user wrote *as the argument* means the program's, and it used to be
;; rewritten too, because the relinker matched by spelling. So `v` is a pair
;; and its second element is not.
;;
;; Fixed 2026-08-26: the relinker renames only identifiers carrying the
;; expansion's own scope, which the expander puts on what a template introduces
;; and on nothing that arrived through a pattern variable.
(define v (both (list 1 2)))

(test-equal "relinking leaves the user's code inside a macro call alone"
  (r7:list #t #f)
  (r7:list (r7:pair? v) (r7:pair? (r7:cadr v))))

;; Inside a quasiquote, `(quote b)` is two symbols of data, and an `unquote`
;; within it is still evaluated. The first fix rewrote such a head to the
;; relinked `quote` at any depth — leaving a `quote.N` symbol in the data — and
;; the template compiler had always inserted the quoted datum verbatim, so
;; `qq2`'s `,(helper 7)`, a library-private reference, was never relinked at
;; all. The vector case is the same claim through `list->vector`.
(test-equal "a quote inside a quasiquote template is data, its unquotes evaluated"
  (r7:list (r7:list 'a (r7:list 'quote 'b) 1)
           (r7:list 'a (r7:list 'quote (r7:list 'b 42)) 1)
           (r7:vector 'a (r7:list 'quote 'b) 1))
  (r7:list (qq 1) (qq2 1) (qq3 1)))

;; A `quote` that reached a template through a pattern variable and was then
;; relinked is still `quote`: the relinker classifies a head by what it
;; *resolves to*, not by its spelling, so the datum after a `quote.N` head is
;; left alone. Pre-existing — it walked the datum and renamed its `list`, so
;; the answer came back as `(list.7 1)`.
(test-equal "a relinked quote head still protects its datum"
  (r7:list 'list 1)
  (outer))

;; Symbols under an ellipsis escape are references like any other: a
;; library-private `helper` inside `(... (helper 1))` resolves where the macro
;; was written, and a generated macro's `(tag x ...)` reaches the library's
;; `tag`. The escape compiler used to emit every non-pattern symbol as a bare
;; literal, so nothing under `(... …)` was hygienic; the first occurrence is
;; what the by-spelling relinker had covered by accident, the second never
;; worked. See the header for why `mk` takes a name.
(mk g)
(def-tagger t)

(test-equal "references under an ellipsis escape resolve at the definition site"
  (r7:list 5 (r7:list 'tagged 1 2))
  (r7:list (g) (t 1 2)))

;; ── An object embedded in evaluated code is not copied ──────────────────────
;;
;; A vector reached through `(eval (list 'outer-mut vec) …)` is the object the
;; expansion mutates, not a copy. The scope flip walks vectors — so that a
;; quasiquoted `#(,(helper x))` can be relinked at all — and its first version
;; copied every vector it walked; it copies only one whose elements changed.
;;
;; **Scoped, because the premise is ours.** R7RS §6.12 makes
;; `interaction-environment`'s bindings implementation-defined, and the two
;; oracles decline for two different reasons, measured 2026-09-09: Gauche's
;; does not contain this script's macros ("unbound variable: outer-mut"), and
;; chibi refuses the mutation outright ("vector-set!: immutable vector"). A
;; **reported skip** rather than a bare `cond-expand`, so the row cannot vanish
;; quietly — and it is what keeps Gauche able to run the file, since the
;; unbound variable would otherwise take the whole thing down and cost the five
;; rows above their only oracle.
;; `vector-set!` and `vector` unprefixed: SRFI 101 does not export either, so
;; these are `(scheme base)`'s without help. That matters for `inner`, whose
;; template is expanded *inside* `eval` — reaching a prefixed name there would
;; make the row depend on what `interaction-environment` carries, which is the
;; one thing about this program the report leaves open. Only the form handed to
;; `eval` needs `r7:list`, since `eval` takes a pair.
(define-syntax inner (syntax-rules () ((_ x) (vector-set! x 0 'changed))))
(define-syntax outer-mut (syntax-rules () ((_ x) (inner x))))

;; `vec` is bound by a `let` rather than defined at top level, which is the one
;; place this file departs from "keep top-level `define`s at top level": the
;; `eval` has to sit *inside* the row so that `test-skip` suppresses it too.
;; At top level it would run on every implementation and take the file down on
;; Gauche. Nothing here is recursive, so the rule the convention protects —
;; a self-reference resolving through the global environment — is not in play.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a vector object in evaluated code keeps its identity through expansion"
  (vector 'changed 2)
  (let ((vec (vector 1 2)))
    (eval (r7:list 'outer-mut vec) (interaction-environment))
    vec))

(test-end)
