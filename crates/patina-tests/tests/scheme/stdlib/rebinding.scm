;; Code compiled before a name is redefined or imported again follows the new
;; binding.
;;
;; R7RS §5.2 makes three things an error in a program or a library: to
;; redefine an imported binding, to import one identifier with two different
;; bindings, and to refer to an identifier before it is imported. It asks a
;; REPL to permit all three. So no answer below is wrong. Every row an oracle
;; answers differently is `latitude` in `DIVERGENCES.tsv` (#603), but for the
;; definition that never ran, about which the standard says nothing, which is
;; `spec-silent`.
;;
;; The rows assert Patina's answer today: a compiled reference to a top-level
;; name follows whatever the name is bound to when it runs. That holds for the
;; VM's inline paths too, which deoptimize when the name is rebound;
;; `vm_callprimitive.rs` and `import_modifiers.rs` pin those paths in Rust.
;; chibi and Chez bind a reference when it is compiled, so code compiled
;; before the change keeps the old binding. Gauche resolves a reference the
;; first time it runs and keeps what it found, so code first run after a
;; redefinition follows it, unless the procedure is one Gauche inlines, which
;; it binds when it compiles the call. The GC redesign keeps Patina's answers
;; through its global-cells work and then moves to binding at compile time
;; (`PRD/GC_PRD.md`); that change will flip these rows and their register
;; entries.
;;
;; Each row rebinds a name of its own, which nothing else in the file uses, so
;; a rebinding reaches no other row. Each rebinds by a definition or an import
;; in this program, never by an assignment, so the locations the libraries
;; share (SRFI 64's included) are untouched. The shapes that need a library
;; are in `library-bindings.scm`.

(import (scheme base) (scheme eval) (scheme repl) (srfi 64))

(test-begin "rebinding")

;; The control. Code compiled after a redefinition uses it on every
;; implementation, so what the rows after it differ on is only code compiled
;; before the change. (It also gives chibi, which differs on every other row,
;; a passing row, which the oracle lane needs to see that the file completed.)
(define (string->vector s) 'mine)
(define (to-vector s) (string->vector s))
(test-equal "a call compiled after a redefinition follows it" 'mine
  (to-vector "ab"))

;; ─── A program redefines a name its compiled code calls ─────────────────────

;; `string-copy` is a primitive: the VM calls it through `CallPrimitive`.
(define (copy-of s) (string-copy s))
(define copied-before (copy-of "ab"))
(define (string-copy s) 'mine)
(test-equal "a primitive call run before a redefinition follows it"
  '("ab" mine)
  (list copied-before (copy-of "ab")))

;; `vector?` is one of the VM's inline opcodes.
(define (is-vector? x) (vector? x))
(define vector-before (is-vector? (vector 1)))
(define (vector? x) 'mine)
(test-equal "an inline-compiled call run before a redefinition follows it"
  '(#t mine)
  (list vector-before (is-vector? (vector 1))))

;; Never run before the redefinition: Gauche follows the new definition here,
;; since it resolves `list-copy` on the first call.
(define (copy-list l) (list-copy l))
(define (list-copy l) 'mine)
(test-equal "a call first run after a redefinition follows it" 'mine
  (copy-list '(1 2)))

;; ─── A program imports a name again from another binding ────────────────────

;; `null?` is an inline opcode; the program imports `pair?` over it.
(define (empty? x) (null? x))
(define empty-before (empty? '()))
(import (rename (only (scheme base) pair?) (pair? null?)))
(test-equal "an inline-compiled call run before a re-import follows it"
  '(#t #f #f)
  (list empty-before (empty? '()) (null? '())))

;; The same, never run before the import. `pair?` is an inline opcode too,
;; which is what makes Gauche bind it when it compiles the call.
(define (is-pair? x) (pair? x))
(import (rename (only (scheme base) list?) (list? pair?)))
(test-equal "an inline-compiled call first run after a re-import follows it"
  '(#t #t)
  (list (is-pair? '()) (pair? '())))

;; ─── A definition that never ran ────────────────────────────────────────────

;; The `define` is never reached, so Patina leaves `vector->list` as it was.
;; chibi and Gauche have already rebound it when the error stops the `begin`:
;; chibi to an unassigned variable, Gauche to an unbound one. The row asks
;; `procedure?` rather than calling the name, because chibi compiles a call
;; of `vector->list` to an opcode and would never look at the binding. `let*`
;; orders the two: chibi evaluates a call's arguments right to left.
(test-equal "a definition the evaluation never reached does not rebind the name"
  '(caught #t)
  (let* ((defined (guard (e (#t 'caught))
                    (eval '(begin (error "boom") (define vector->list 5))
                          (interaction-environment))))
         (still (guard (e (#t 'caught))
                  (eval '(procedure? vector->list) (interaction-environment)))))
    (list defined still)))

;; ─── The control sites the VM compiles specially ────────────────────────────
;;
;; Last, because it rebinds two names the expansions of `guard`, `let-values`
;; and others refer to. Those references reach the library's bindings, not
;; these, but a row after this one would depend on that.
;;
;; The `dynamic-wind` thunks have effects on purpose: Gauche 0.9.15 returns
;; the after thunk itself, uncalled, from a `dynamic-wind` whose before and
;; after thunks are lambdas with constant bodies (measured 2026-10-07; the
;; `stdlib/scheme-r5rs.scm` row fails on Gauche with the same shape), which
;; would hide what this row asks.
(define winds 0)
(define (values-site) (call-with-values (lambda () (values 1 2)) list))
(define (wind-site)
  (dynamic-wind (lambda () (set! winds (+ winds 1)))
                (lambda () 'body)
                (lambda () (set! winds (+ winds 1)))))
(define sites-before (list (values-site) (wind-site)))
(define (call-with-values producer consumer) 'mine)
(define (dynamic-wind before thunk after) 'mine)
(test-equal "the call-with-values and dynamic-wind sites follow a redefinition"
  '(((1 2) body) mine mine 2)
  (let* ((values-after (values-site)) (wind-after (wind-site)))
    (list sites-before values-after wind-after winds)))

(test-end)
