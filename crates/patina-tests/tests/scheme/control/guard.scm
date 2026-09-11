;; `guard`, and exception handlers nested inside each other.
;;
;; Migrated from `crates/patina-tests/tests/nested_exception_handlers.rs`
;; (#193 Phase 2), which is deleted. Its subject began as a tree-walker defect:
;; `with-exception-handler` wraps its thunk's continuation in an
;; `ExceptionHandlerCleanup` so the handler is popped on the way out, and the
;; continuation serializer had no representation for that wrapper — or for the
;; five others like it — so re-entering a continuation captured under one
;; failed with `Undefined variable: k_N`. `guard` expands to `call/cc` +
;; `with-exception-handler`, so any `guard` nested inside another handler
;; tripped it: 15 tests of the R7RS suite, invisible until Patina adopted
;; upstream `(chibi test)`, whose applier calls each test thunk from inside
;; its own `guard`. The VM was never affected — it snapshots machine state
;; rather than serializing continuations name by name.
;;
;; Two of its tests asserted a *leaked* handler through an unguarded
;; program's final error. A leaked handler is just as visible to a `guard`
;; placed around the leaking call — it sits above the guard's own and
;; intercepts the error first — so those are rows here too, and the file is
;; whole.
;;
;; Divergences are recorded in `DIVERGENCES.tsv`, not restated here.

(import (scheme base) (srfi 64))

(test-begin "guard")

;; ── Every clause shape ─────────────────────────────────────────────────────
;;
;; Every shape of `guard` R7RS §4.2.7 defines, in one program, checked against
;; chibi and Gauche — which produce this list element for element — except
;; the clause-less form, which has a row of its own below.
;;
;; `guard` was rewritten on 2026-09-01 to R7RS 7.3's expansion, bar one
;; deliberate deviation on the success path (`lib/scheme/base/exceptions.scm`
;; says why, and `prompts.scm` pins it). The expansion routes the body's
;; *normal* result through a `call-with-values` and a thunk, so the shapes at
;; risk are not only the exception ones: multiple values, zero values, and a
;; body whose definitions must stay in a body context. The clause walker is
;; its own `%guard-aux`, which puts `else` and `=>` at risk, and `var` is
;; bound by a `let` inside a template, which puts hygiene at risk.
(test-equal "guard covers every R7RS clause shape"
  '((else a) 42 (2 3) fell-through fell-through 5 str (outer inner) 3
    (1 2 3) () ok 42 inner (top obj))
  (let ((out '()))
    (define (show x) (set! out (cons x out)))
    (show (guard (e (else (list 'else e))) (raise 'a)))
    (show (guard (e ((assq 'b e) => cdr)) (raise (list (cons 'b 42)))))
    (show (guard (e ((memv e '(1 2 3)))) (raise 2)))
    ;; the two NON-terminal walker rules: `=>` and a bare test, each with a
    ;; clause after them, which the terminal rules would otherwise hide
    (show (guard (e ((assq 'b e) => cdr) (#t 'fell-through)) (raise (list (cons 'c 1)))))
    (show (guard (e ((memv e '(7 8))) (#t 'fell-through)) (raise 9)))
    (show (guard (e ((assq 'b e) => cdr) (#t 'no)) (raise (list (cons 'b 5)))))
    (show (guard (e ((symbol? e) 'sym) ((string? e) 'str)) (raise "s")))
    (show (guard (o (#t (list 'outer o)))
            (guard (e ((string? e) 'str)) (raise 'inner))))
    (show (guard (e (#t 'never)) 1 2 3))
    (show (call-with-values (lambda () (guard (e (#t 'never)) (values 1 2 3))) list))
    (show (call-with-values (lambda () (guard (e (#t 'never)) (values))) list))
    (show (let ((else #f)) (guard (e ((symbol? e) 'sym)) 'ok)))
    (show (guard (e (#t 'never)) (define x 7) (* x 6)))
    (show (let ((e 'outer)) (guard (e (#t e)) (raise 'inner))))
    (show (guard (a (#t (list 'top a)))
            (guard (b ((string? b) 'no))
              (guard (c ((number? c) 'no))
                (raise 'obj)))))
    (reverse out)))

;; No clauses at all — R7RS 7.1.3 allows it (`(guard (<identifier> <cond
;; clause>*) <body>)`), and it means "re-raise", on both raise forms. Gauche
;; agrees. chibi 0.12 refuses to *expand* it ("no expansion for: (guard-aux
;; …)"), which loses the whole file rather than the row, and a `test-skip`
;; cannot help — a skipped row is still compiled — so the row sits in a
;; `cond-expand` clause chibi never selects. Whether chibi compiles a program
;; is chibi's conformance, not a claim of ours that could drift.
(cond-expand
  (chibi)
  (else
   (test-equal "a guard with no clauses re-raises" '((outer x) (body (h y)))
     (list (guard (o (#t (list 'outer o))) (guard (e) (raise 'x)))
           (with-exception-handler (lambda (c) (list 'h c))
             (lambda () (guard (e) (list 'body (raise-continuable 'y)))))))))

;; ── Nested handlers ────────────────────────────────────────────────────────

(test-equal "a guard nested through a thunk" '(ok inner)
  (let ((run (lambda (th) (guard (e (#t (list 'outer e))) (list 'ok (th))))))
    (run (lambda () (guard (x (else 'inner)) (error "boom"))))))

(test-equal "a guard nested without an intervening procedure" '(ok inner)
  (guard (e (#t 'outer))
    (list 'ok (guard (x (else 'inner)) (error "boom")))))

;; The inner clause does not match, so the outer handler must still fire —
;; the continuation is restored, not the raise swallowed.
(test-equal "an inner guard declines and the outer one handles" '(outer 42)
  (guard (e (#t (list 'outer e)))
    (list 'ok (guard (x ((symbol? x) 'inner)) (raise 42)))))

(test-equal "three levels of nesting" '(l1 7)
  (guard (a (#t (list 'l1 a)))
    (list 'x (guard (b ((string? b) 'l2))
               (list 'y (guard (c ((symbol? c) 'l3)) (raise 7)))))))

;; The underlying shape, with no `guard` involved: a non-tail `call/cc` inside
;; a `with-exception-handler` thunk. In tail position the wrapper is the
;; continuation itself and was already handled, which is why the value has to
;; be consumed by an enclosing form. `CallWithValuesConsumer` is one of the
;; other five wrappers that were being dropped: same defect, different
;; variant.
(test-equal "a non-tail call/cc inside a handler thunk and a producer" '((ok jump) (ok j))
  (list (with-exception-handler (lambda (c) 'outer)
          (lambda () (list 'ok (call-with-current-continuation (lambda (k) (k 'jump))))))
        (call-with-values
          (lambda () (list 'ok (call-with-current-continuation (lambda (k) (k 'j)))))
          (lambda (a) a))))

;; `dynamic-wind` had its own serialization and always worked; this pins that
;; the shared unwrap path did not disturb it.
(test-equal "a guard under dynamic-wind" 'caught
  (dynamic-wind (lambda () 1)
                (lambda () (guard (e (else 'caught)) (error "x")))
                (lambda () 3)))

;; ── A guard inside a primitive's callback ──────────────────────────────────
;;
;; The success path's deliberate deviation from R7RS 7.3, pinned from one of
;; its two sides: a `guard` whose body returns normally inside a primitive's
;; callback must leave that callback running. The reference line jumps to
;; `guard-k` even on success; until 2026-09-10 every such jump read as an
;; escape from the tree-walker's nested trampoline, and `call-with-port`
;; closed the port under a callback that then read from it. That reason is
;; gone, and the deviation stays for a better one — the jump is wrong when the
;; body is resumed through a composable continuation — which `prompts.scm`
;; pins ("a composable continuation resumed through a successful guard
;; returns to its invoker").
(test-equal "a guard that succeeds inside a callback leaves the callback's port open" #\b
  (call-with-port (open-input-string "abc")
    (lambda (p)
      (guard (e (#t 'no)) (read-char p))
      (read-char p))))

;; ── A handler is popped when its thunk returns ─────────────────────────────
;;
;; `with-exception-handler` pops its handler when its thunk returns — and a
;; thunk that ends in a tail call returns through a different VM path per
;; callee. The VM popped only on `Return`, which a closure callee reaches; a
;; control primitive, `values`, a primitive and a parameter each delivered
;; straight to the caller and left the handler installed for the rest of the
;; program. `guard`'s reference expansion ends its success path in `(apply
;; values args)`, so every successful `guard` under a `with-exception-handler`
;; leaked (found by review of triage families 22/28, 2026-09-01).
;;
;; A leaked handler sits above anything installed around the leaking call, so
;; the probe is an unrelated error afterwards, under a `guard`: a leak turns
;; `car`'s error into `leaked` before the guard sees it.
(define (leak thunk) (with-exception-handler (lambda (e) (raise 'leaked)) thunk))
(define (probe-leak thunk)
  (guard (e (#t (if (eq? e 'leaked) 'leaked 'car-error)))
    (leak thunk)
    (car 5)))

(test-equal "a handler thunk ending in a tail call still pops its handler"
  '(car-error car-error car-error car-error car-error)
  (let ((p (make-parameter 1)))
    (map probe-leak
         (list (lambda () (values 1))                 ; TailCallWithValues
               (lambda () (guard (e (#f 'no)) 'fine)) ; (apply values args) ending a guard
               (lambda () (car '(1)))                 ; a primitive
               (lambda () (p))                        ; a parameter
               (lambda () (call/cc (lambda (k) 1))))))) ; a control primitive

;; The same pop where the thunk returns to a *nested* run loop — a
;; `call-with-port` callback, or the body of `dynamic-wind` reached as a
;; value — rather than to a frame. That return does not go through the
;; frame-depth test at all: the loop closes the handlers installed under it
;; from its own entry count, because at its exit depth the frame-depth test
;; cannot tell a handler it was started under from one installed inside it
;; (the next row is the other side of that ambiguity).
(test-equal "a handler installed inside a nested run is popped when the run returns"
  '(car-error car-error)
  (let ((dw dynamic-wind))
    (list (guard (e (#t (if (eq? e 'leaked) 'leaked 'car-error)))
            (call-with-port (open-input-string "a") (lambda (port) (leak (lambda () 'x))))
            (car 5))
          (guard (e (#t (if (eq? e 'leaked) 'leaked 'car-error)))
            (dw (lambda () #f) (lambda () (leak (lambda () 'x))) (lambda () #f))
            (car 5)))))

;; The other side: a handler that a nested run loop was *started under* must
;; survive that run. The thunk tail-calls `dynamic-wind` as a value, so its own
;; frame is gone by the time `before` runs on a nested loop — the handler sits
;; at exactly that loop's exit depth, indistinguishable by depth from one
;; whose thunk has returned, and still owed `body`'s raise. The first review
;; fix for the leak above popped it at `before`'s return, and this lost its
;; handler. chibi and Gauche agree.
(test-equal "a handler survives a nested run its thunk's tail call started"
  '(handled (in handler out))
  (let ((dw dynamic-wind) (v '()))
    (define (log x) (set! v (cons x v)))
    (let ((answer (with-exception-handler
                    (lambda (e) (log 'handler) 'handled)
                    (lambda ()
                      (dw (lambda () (log 'in))
                          (lambda () (raise-continuable 'x))
                          (lambda () (log 'out)))))))
      (list answer (reverse v)))))

;; ── Where a handler and a clause run ───────────────────────────────────────

;; The handler and the `guard` clauses run in *different* dynamic
;; environments, and a parameter says which more directly than a wind log
;; does. chibi and Gauche produce this exactly. R7RS 6.11 runs the handler "in
;; the dynamic environment of the call to raise", so it reads `inner`; R7RS
;; 4.2.7 runs the clauses "in the dynamic environment of the guard
;; expression", so they read `outer` — `guard-k` has already left the
;; `parameterize` by then. Before triage families 22 and 28 the raise path
;; unwound first and the handler read `outer` too, which collapsed the
;; distinction this row exists to hold.
(test-equal "a handler and a guard clause run in different dynamic environments"
  '((clause outer) (handler inner) (nested mid))
  (let ((p (make-parameter 'outer)))
    (list
      (guard (e (#t (list 'clause (p)))) (parameterize ((p 'inner)) (raise 'x)))
      (with-exception-handler (lambda (e) (list 'handler (p)))
        (lambda () (parameterize ((p 'inner)) (raise-continuable 'x))))
      (parameterize ((p 'mid)) (guard (e (#t (list 'nested (p)))) (raise 'x))))))

;; A `guard` whose clauses all fail re-raises **in the raiser's dynamic
;; extent** (R7RS §4.2.7), so a `dynamic-wind` between the two guards is
;; re-entered before the re-raise and exited again after it. Fixed 2026-09-01
;; with Track L triage families 22 and 28.
;;
;; Patina used to re-raise where the `guard` stands, on both backends — a
;; deviation from chibi rather than a divergence, and observable only with
;; side-effecting wind thunks (audit F9), because the expression's value is the
;; same either way. This pin outlived two wrong diagnoses, both worth keeping:
;; "the fix is `guard`'s expansion, not the handler machinery" was wrong —
;; R7RS 7.3's reference `guard` still gave `(in out)`, because the raise path
;; unwound before calling the handler, so the continuation it carries back
;; was captured after the extent had been left; and "so it is the handler
;; machinery, not `guard`" was wrong too. It took four changes together: the
;; handler stack on `CpsContinuation` (#150), the VM's wind common prefix
;; (#149), no unwind on any raise path, and the reference `guard`.
(test-equal "a guard's re-raise rewinds into the raiser"
  '((outer boom) (in out in out))
  (let ((log '()))
    (let ((result (guard (e ((symbol? e) (list 'outer e)))
                    (guard (e ((string? e) 'inner))
                      (dynamic-wind
                        (lambda () (set! log (cons 'in log)))
                        (lambda () (raise 'boom))
                        (lambda () (set! log (cons 'out log))))))))
      (list result (reverse log)))))

(test-end)
