;; CPS-dependent control flow — exception handler stacks, nested `dynamic-wind`,
;; continuation capture, `guard`, and the routing of runtime errors.
;;
;; Migrated from `crates/patina-tests/tests/cps_features.rs` (#193 Phase 1),
;; which is being split rather than moved whole. That file had three parts with
;; different portability:
;;
;;   - **this file** — 50 rows at the time, 45 of plain R7RS control flow plus
;;     Larceny family 27's four and one continuation-as-handler row moved in
;;     from `larceny_families.rs`; 82 now, with `backend_divergence.rs`'s rows
;;     (the last four sections, see below);
;;   - the delimited-continuation half — `make-continuation-prompt-tag`,
;;     `call-with-continuation-prompt`, `abort-current-continuation` — which
;;     migrates separately, because measured 2026-09-07 all three procedures are
;;     **absent from both chibi and Gauche**, so those rows can never have an
;;     oracle and do not belong beside rows that do;
;;   - one test that stays in Rust: it spawns a thread with an explicit
;;     `stack_size` to drive `dynamic-wind` 100_000 deep. Choosing a stack size
;;     is a harness act, not something a Scheme program can ask for.
;;
;; The `.rs` rows compared printed forms through `assert_program_eval_to`. These
;; are lists, numbers, symbols and strings throughout, where `test-equal` on
;; values says the same thing more directly — and `equal?` is type-sensitive, so
;; nothing here needs the `written` helper the printing-heavy files carry.
;;
;; Divergences are recorded in `DIVERGENCES.tsv` and checked by
;; `scripts/run_suite_oracles.sh`, not restated here.
;;
;; The last four sections came from `crates/patina-tests/tests/
;; backend_divergence.rs` (#193), the registry of behaviours where the two
;; backends differed — most of them converged, each row keeping the account of
;; what was wrong and when it was fixed. The rows still open carry a
;; **backend-scoped expectation** — `(cond-expand (patina-tree-walker
;; (test-expect-fail 1)) (else))` above the row — which is how a divergence is
;; written now that each backend advertises its own feature identifier: the
;; row asserts the right answer everywhere, the line says who is known to get
;; it wrong, and the driver fails the run the day that backend starts passing.
;; `docs/TEST_ORGANIZATION.md` has the mechanism.

(import (scheme base) (scheme write) (scheme file) (scheme read)
        (scheme eval) (scheme repl) (srfi 64))

;; `(scheme stream)` is imported only where it is used. It backs one row, that
;; row is scoped to Patina, and it is R7RS-large Red rather than R7RS-small —
;; so importing it unconditionally would put every other row at the mercy of
;; a library none of them touch, on any implementation that lacks SRFI 41.
(cond-expand (patina (import (scheme stream))) (else))

(test-begin "cps-features")

;; ── Exception handler stack management ─────────────────────────────────────

(test-equal "a handler catches what the thunk raises" '(caught my-error)
  (call-with-current-continuation
    (lambda (k)
      (with-exception-handler
        (lambda (e) (k (list 'caught e)))
        (lambda () (raise 'my-error))))))

(test-equal "an installed handler that never fires costs nothing" 6
  (with-exception-handler (lambda (e) 'never-called) (lambda () (+ 1 2 3))))

(test-equal "the innermost handler wins" 'inner
  (call-with-current-continuation
    (lambda (escape)
      (with-exception-handler
        (lambda (e) (escape 'outer))
        (lambda ()
          (with-exception-handler
            (lambda (e) (escape 'inner))
            (lambda () (raise 'boom))))))))

;; The handler stack has to *pop* when the thunk returns normally, or a later
;; raise finds a handler that is no longer installed.
(test-equal "a handler is popped when its thunk returns" '(outer after-inner)
  (call-with-current-continuation
    (lambda (escape)
      (with-exception-handler
        (lambda (e) (escape (list 'outer e)))
        (lambda ()
          (with-exception-handler
            (lambda (e) (escape (list 'inner e)))
            (lambda () 'ok))
          ;; the inner handler is gone by here
          (raise 'after-inner))))))

;; `raise-continuable` is the one that lets the handler's return value become
;; the value of the raise expression: 1 + (5 * 10).
(test-equal "raise-continuable resumes with the handler's value" 51
  (with-exception-handler (lambda (e) (* e 10))
    (lambda () (+ 1 (raise-continuable 5)))))

;; Three in one expression: 101 + 102 + 103. The handler must survive all three
;; rather than being consumed by the first.
(test-equal "and the handler survives repeated raises" 306
  (with-exception-handler (lambda (e) (+ e 100))
    (lambda () (+ (raise-continuable 1) (raise-continuable 2) (raise-continuable 3)))))

;; ── dynamic-wind, nested ───────────────────────────────────────────────────

(test-equal "before, body and after all run" '(before body after)
  (let ((log '()))
    (dynamic-wind
      (lambda () (set! log (cons 'before log)))
      (lambda () (set! log (cons 'body log)) 'result)
      (lambda () (set! log (cons 'after log))))
    (reverse log)))

(test-equal "two levels nest in the obvious order"
  '(outer-before inner-before body inner-after outer-after)
  (let ((log '()))
    (dynamic-wind
      (lambda () (set! log (cons 'outer-before log)))
      (lambda ()
        (dynamic-wind
          (lambda () (set! log (cons 'inner-before log)))
          (lambda () (set! log (cons 'body log)) 'result)
          (lambda () (set! log (cons 'inner-after log)))))
      (lambda () (set! log (cons 'outer-after log))))
    (reverse log)))

(test-equal "and so do three" '(a-in b-in c-in body c-out b-out a-out)
  (let ((log '()))
    (dynamic-wind
      (lambda () (set! log (cons 'a-in log)))
      (lambda ()
        (dynamic-wind
          (lambda () (set! log (cons 'b-in log)))
          (lambda ()
            (dynamic-wind
              (lambda () (set! log (cons 'c-in log)))
              (lambda () (set! log (cons 'body log)) 'result)
              (lambda () (set! log (cons 'c-out log)))))
          (lambda () (set! log (cons 'b-out log)))))
      (lambda () (set! log (cons 'a-out log))))
    (reverse log)))

;; R7RS requires the after thunk to run on a non-local exit, which a raise is.
(test-equal "a raise in the body still runs the after thunk" '(before body after)
  (let ((log '()))
    (guard (e (else (reverse log)))
      (dynamic-wind
        (lambda () (set! log (cons 'before log)))
        (lambda () (set! log (cons 'body log)) (raise 'boom))
        (lambda () (set! log (cons 'after log)))))))

(test-equal "so does an escape through call/cc" '(before body after)
  (let ((log '()))
    (call-with-current-continuation
      (lambda (escape)
        (dynamic-wind
          (lambda () (set! log (cons 'before log)))
          (lambda () (set! log (cons 'body log)) (escape 'escaped))
          (lambda () (set! log (cons 'after log))))))
    (reverse log)))

;; Re-entering runs the before thunks again, so the log records two full trips.
(test-equal "re-entry replays the before thunk" '(in body out in body out)
  (let ((log '()) (k #f))
    (dynamic-wind
      (lambda () (set! log (cons 'in log)))
      (lambda ()
        (call-with-current-continuation (lambda (c) (set! k c)))
        (set! log (cons 'body log)))
      (lambda () (set! log (cons 'out log))))
    (if (< (length log) 6) (k #f) (reverse log))))

;; ── Continuation capture ───────────────────────────────────────────────────

;; The `(+ 2 …)` is abandoned: invoking k discards the rest of the body.
(test-equal "invoking the continuation abandons the rest" 11
  (+ 1 (call-with-current-continuation (lambda (k) (+ 2 (k 10))))))

(test-equal "a continuation never invoked costs nothing" 6
  (call-with-current-continuation (lambda (k) (+ 1 2 3))))

;; First pass: saved gets k, returns 10, result 11, 11 < 20 so re-enter with 50.
;; Second pass: result 51, which is >= 20.
(test-equal "a stored continuation can be invoked later" 51
  (let ((saved #f))
    (let ((result (+ 1 (call-with-current-continuation
                         (lambda (k) (set! saved k) 10)))))
      (if (< result 20) (saved 50) result))))

(test-equal "and invoked more than once" 3
  (let ((k #f) (count 0))
    (+ 1 (call-with-current-continuation (lambda (c) (set! k c) 0)))
    (set! count (+ count 1))
    (if (< count 3) (k count) count)))

(test-equal "nested captures compose" 11
  (call-with-current-continuation
    (lambda (outer)
      (+ 1 (call-with-current-continuation
             (lambda (inner) (outer (inner 10))))))))

;; Top level rather than wrapped in a `let`: a top-level self-reference resolves
;; through the global environment, and an internal define would be a local slot.
(define (tail-callcc)
  (call-with-current-continuation (lambda (k) (k 42))))
(test-equal "call/cc in tail position" 42 (tail-callcc))

;; These three came from the `.rs` file's "instruction-level control ops"
;; sections, which sat below the prompt tests and were nearly left behind with
;; them. They use nothing but `call/cc` and `dynamic-wind`.
;;
;; A fourth, `test_dynamic_wind_callcc_escape_runs_after`, is **not** here: it
;; is the same program as "so does an escape through call/cc" above, differing
;; only in the value handed to `escape`, which is discarded. The `.rs` file
;; asserted it twice.

(test-equal "a single value through call-with-values" 42
  (call-with-values
    (lambda () (call-with-current-continuation (lambda (k) (k 42))))
    (lambda (x) x)))

;; Re-entry delivers the value the continuation was *given*, not the one the
;; original capture returned: `first` on the way through, `second` on re-entry.
(test-equal "re-entry delivers the value it was handed" '(first second)
  (let ((k #f) (results '()))
    (let ((val (dynamic-wind
                 (lambda () #f)
                 (lambda () (call-with-current-continuation
                              (lambda (c) (set! k c) 'first)))
                 (lambda () #f))))
      (set! results (cons val results))
      (when (and k (< (length results) 3))
        (let ((saved k)) (set! k #f) (saved 'second))))
    (reverse results)))

(test-equal "before and after run on every entry and exit" '(in out in out in out)
  (let ((k #f) (log '()))
    (dynamic-wind
      (lambda () (set! log (cons 'in log)))
      (lambda ()
        (if (not k) (call-with-current-continuation (lambda (c) (set! k c))))
        'ok)
      (lambda () (set! log (cons 'out log))))
    (if (< (length log) 6) (k 'again))
    (reverse log)))

;; ── guard ──────────────────────────────────────────────────────────────────

(test-equal "clauses are tried in order" '(number 42)
  (guard (e ((number? e) (list 'number e))
            ((symbol? e) (list 'symbol e))
            (else (list 'other e)))
    (raise 42)))

(test-equal "a clause condition is an ordinary expression" 100
  (guard (e ((and (list? e) (= (length e) 2)) (cadr e))
            (else 'no-match))
    (raise (list 'data 100))))

(test-equal "with no matching clause, guard re-raises outward" '(outer symbol)
  (call-with-current-continuation
    (lambda (escape)
      (with-exception-handler
        (lambda (e) (escape (list 'outer e)))
        (lambda ()
          (guard (inner-e ((number? inner-e) 'number))
            (raise 'symbol)))))))

(test-equal "a guard body is a body, so it takes internal defines" 30
  (guard (e (else 'error))
    (define x 10)
    (define y 20)
    (+ x y)))

;; ── Handlers and dynamic-wind together ─────────────────────────────────────

(test-equal "a raise in the before thunk is catchable" '(caught before-error)
  (guard (e (else (list 'caught e)))
    (dynamic-wind (lambda () (raise 'before-error))
                  (lambda () 'body)
                  (lambda () 'after))))

(test-equal "a raise in the body runs the after thunk before the guard sees it"
  '(caught before after)
  (let ((log '()))
    (guard (e (else (cons 'caught (reverse log))))
      (dynamic-wind
        (lambda () (set! log (cons 'before log)))
        (lambda () (raise 'body-error))
        (lambda () (set! log (cons 'after log)))))))

;; The subtle one, and the reason it is spelled out: `(reverse log)` is
;; evaluated *before* `escape` transfers control, so the value carried out is
;; `(before)`. The after thunk still runs and still mutates `log` — it just does
;; so after the escaping value was computed.
(test-equal "an escape argument is evaluated before the after thunk runs"
  '(before)
  (let ((log '()))
    (call-with-current-continuation
      (lambda (escape)
        (dynamic-wind
          (lambda () (set! log (cons 'before log)))
          (lambda () (escape (reverse log)))
          (lambda () (set! log (cons 'after log))))))))

;; ── Error objects ──────────────────────────────────────────────────────────

;; A bare comparison, like its sibling further down. The first draft used
;; `written` here on the grounds that `test-equal` against a string "would hold
;; for a symbol of the same name" — which is false: `(equal? "abc" 'abc)` is
;; `#f`, so the comparison is already type-sensitive and `written` bought
;; nothing but an inconsistency with the other message row.
(test-equal "an error object carries its message" "test message"
  (guard (e ((error-object? e) (error-object-message e)) (else 'not-error))
    (error "test message")))

(test-equal "and its irritants" '(a b c)
  (guard (e ((error-object? e) (error-object-irritants e)) (else 'not-error))
    (error "msg" 'a 'b 'c)))

;; **Larceny family 27** (`scheme_tests/reports/larceny_triage.md`), moved here
;; from `larceny_families.rs` because this is the error-objects section.
;;
;; R7RS §6.11 says the message *should* be a string — advice, not a
;; requirement — and the R6RS habit of `(error 'who "what")` runs through SRFI
;; reference implementations, the bundled SRFI 41 among them: all 53 of its
;; diagnostics were being replaced by a complaint about the argument (found by
;; review of the SRFI 41 bundle, 2026-08-25). A non-string message is now
;; accepted on both backends, in the primitive and in each backend's `error`
;; intercept.
;;
;; The irritants are the portable half — all three implementations agree.
(test-equal "a non-string message keeps its irritants" '(("bar" 1) (2))
  (list (guard (e (#t (error-object-irritants e))) (error 'foo "bar" 1))
        (guard (e (#t (error-object-irritants e))) (error "plain" 2))))

;; A string message is unremarkable and portable — kept because the `.rs` row
;; had it, and because it is the control for the row below.
(test-equal "a string message and its irritants come back unchanged" '("plain" (2))
  (list (guard (e (#t (error-object-message e))) (error "plain" 2))
        (guard (e (#t (error-object-irritants e))) (error "plain" 2))))

;; The *symbol* message is where we differ, and it gets a row to itself so a
;; failure names the behaviour rather than printing two lists to diff. Measured
;; 2026-09-07: Patina answers the **string** `"foo"`; chibi and Gauche both
;; answer the **symbol** `foo`. R7RS says `error-object-message` returns the
;; message, and says the message should be a string, without saying what
;; happens when it is not — so converting and passing through are both
;; defensible. Left unscoped and registered, because a recorded difference is
;; worth more than a row that vanishes.
(test-equal "a symbol message comes back as a string" "foo"
  (guard (e (#t (error-object-message e))) (error 'foo "bar" 1)))

;; **Scoped to Patina: the premise is about our SRFI 41 bundle.** This is the
;; case family 27 was actually found by — `(stream-car 5)` reaches
;; `(error 'stream-car …)` inside the bundled SRFI 41, and before the fix that
;; diagnostic, and 52 others like it, were replaced wholesale by a complaint
;; about the argument. Every implementation raises here; only the message is
;; ours. Measured 2026-09-07: chibi says "slot-ref: bad type" and Gauche "pair
;; required, but got 5", each its own diagnostic from its own stream code, so
;; there is nothing portable to assert and no corroboration to lose.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a library's own diagnostic survives to error-object-message"
  "stream-car"
  (guard (e (#t (error-object-message e))) (stream-car 5)))

(test-equal "a raised symbol is not an error object" 'not-error
  (guard (e ((error-object? e) 'is-error) (else 'not-error))
    (raise 'plain-symbol)))

;; ── Combinations ───────────────────────────────────────────────────────────

;; After a normal exit the log reads `(out in)`, so `(car log)` is `out` and the
;; re-entry branch is not taken. The `.rs` file records that its original
;; expectation here was wrong and was corrected against chibi.
(test-equal "a continuation captured inside a wind, not re-entered" '(done in out)
  (let ((k #f) (log '()))
    (dynamic-wind
      (lambda () (set! log (cons 'in log)))
      (lambda () (call-with-current-continuation (lambda (c) (set! k c) 'first)))
      (lambda () (set! log (cons 'out log))))
    (if (eq? (car log) 'in) (k 'second) (cons 'done (reverse log)))))

(test-equal "re-entry replays before thunks until the count is reached" 3
  (let ((k #f) (count 0))
    (dynamic-wind
      (lambda () (set! count (+ count 1)))
      (lambda () (call-with-current-continuation (lambda (c) (set! k c))))
      (lambda () #f))
    (if (< count 3) (k #f) count)))

(test-equal "a handler escaping through a stored continuation" '(caught error)
  (call-with-current-continuation
    (lambda (escape)
      (with-exception-handler
        (lambda (e) (escape (list 'caught e)))
        (lambda () (raise 'error))))))

;; ── Runtime errors route through handlers ──────────────────────────────────
;;
;; R7RS does not require these conditions to be signalled as *error objects* —
;; §6.11 leaves most of them "an error" without saying what is raised. These
;; rows pin that Patina raises something `error-object?` accepts, and where an
;; oracle answers otherwise the register says so.

(test-equal "a type error" 'caught-type-error
  (guard (ex ((error-object? ex) 'caught-type-error)) (+ "not-a-number" 1)))

(test-equal "an unbound variable" 'caught-undefined
  (guard (ex ((error-object? ex) 'caught-undefined)) undefined-variable-xyz))

;; **The indirection through `arity-victim` is load-bearing, and not ours.**
;; Gauche detects an arity violation *statically* wherever it can see both the
;; procedure and the argument count, and reports it while compiling — which no
;; `guard` can catch, and which is not even a row failure: it kills the file,
;; taking every row after it. Measured 2026-09-07, the literal `(car 1 2 3)`
;; from the `.rs` file does that, and so does `(apply car (list 1 2 3))`,
;; because Gauche sees through the `apply` too. A `test-skip` cannot help,
;; since the form is compiled before any skip could apply.
;;
;; Behind a variable it is a runtime error, and then **Gauche catches it like
;; everyone else** — so the difference is about *when* the violation is
;; detected, not whether it is catchable. That correction is worth stating,
;; because the first reading of the evidence here was "Gauche cannot catch
;; arity errors at all", and that is false.
(define arity-victim car)
(test-equal "an arity error" 'caught-arity
  (guard (ex ((error-object? ex) 'caught-arity)) (apply arity-victim (list 1 2 3))))

(test-equal "division by zero" 'caught-division
  (guard (ex ((error-object? ex) 'caught-division)) (/ 1 0)))

(test-equal "an index out of bounds" 'caught-bounds
  (guard (ex ((error-object? ex) 'caught-bounds)) (vector-ref (vector 1 2 3) 100)))

(test-equal "applying a non-procedure" 'caught-app
  (guard (ex ((error-object? ex) 'caught-app)) (42 1 2)))

(test-equal "and the same through with-exception-handler" 'caught
  (call-with-current-continuation
    (lambda (escape)
      (with-exception-handler
        (lambda (ex) (escape (if (error-object? ex) 'caught 'not-error)))
        (lambda () (car "not-a-pair"))))))

(test-equal "error-object-irritants on an explicit error" '(1 2 3)
  (guard (ex ((error-object? ex) (error-object-irritants ex))) (error "test error" 1 2 3)))

;; `file-error?` and `read-error?` are the two R7RS predicates that classify a
;; raised object beyond `error-object?`, so they get rows of their own.
(test-equal "a missing file raises a file-error" 'file-error
  (guard (ex ((file-error? ex) 'file-error) ((error-object? ex) 'other-error))
    (open-input-file "/nonexistent/path/that/does/not/exist")))

(test-equal "malformed input raises a read-error" 'read-error
  (guard (ex ((read-error? ex) 'read-error) ((error-object? ex) 'other-error))
    (read (open-input-string "1.2.3"))))

;; **Scoped to Patina, because its premise is.** The text of a runtime error
;; message is ours to choose — R7RS says nothing about it — so this row pins our
;; wording rather than a portable claim, and elsewhere reports a skip rather
;; than a difference.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "our type-error message survives into the error object"
  "car expects a pair"
  (guard (ex ((error-object? ex) (error-object-message ex))) (car 'not-a-pair)))


;; **From `larceny_families.rs`'s "What `base` found once it ran" section.**
;;
;; `with-exception-handler` takes a *continuation* as its handler — R7RS's
;; idiom for capturing a raised object, `(call/cc (lambda (k)
;; (with-exception-handler k …)))`. The VM used to reject it ("expected a
;; procedure, got object") until 2026-08-25: its type check asked for a
;; procedure, and its generic call path could not invoke a continuation.
;;
;; With the object in hand, `read-error?` and `file-error?` answer `#f`, as
;; R7RS §6.11 requires of them for an error that is neither — and answer `#f`
;; for non-objects too, rather than raising.
(test-equal "a continuation is a procedure for with-exception-handler"
  '(#t "plain" #f #f #f #f obj)
  (let ((e (call/cc (lambda (k)
                      (with-exception-handler k (lambda () (error "plain")))))))
    (list (error-object? e) (error-object-message e)
          (read-error? e) (file-error? e) (read-error? 42) (file-error? 'x)
          (call/cc (lambda (k)
                     (with-exception-handler k (lambda () (raise 'obj))))))))

;; ── Continuation escapes, and re-entry with the handler stack ──────────────

;; A `call/cc` continuation invoked with multiple values. Converged
;; 2026-08-25: the tree-walker delivers a `#<values>` object for any count but
;; one, as the VM has since #113 and as `(values …)` itself does.
(test-equal "a multi-value continuation through call-with-values" '(1 2)
  (call-with-values
    (lambda () (call-with-current-continuation (lambda (k) (k 1 2))))
    (lambda (a b) (list a b))))

;; The abort pattern used by SRFI 1's `%cars+cdrs`, and the reason the whole
;; n-ary half of `(scheme list)` was unusable on the tree-walker: `zip`,
;; `fold`, `any`, `every` and `list-index` over two or more lists all reach
;; it. The SRFI 1 procedures this unblocks are asserted once, in
;; `stdlib/list.scm` (Larceny family 5) — not duplicated here.
(test-equal "the SRFI 1 abort pattern through call-with-values"
  '(cars () cdrs ())
  (call-with-values
    (lambda () (call-with-current-continuation (lambda (abort) (abort '() '()))))
    (lambda (cars cdrs) (list 'cars cars 'cdrs cdrs))))

;; An error raised *after* a continuation escape is catchable — converged
;; 2026-09-01 when `CpsContinuation` gained the handler stack. The tree-walker
;; used to abort with `Type error: car expects a pair`, because the escape had
;; emptied the `guard`'s handler stack on the way past.
;;
;; Found through a multi-value shape —
;; `(guard (e (#t (list 'caught))) (+ 1 (call/cc (lambda (k) (k 1 2)))))`,
;; reachable since 2026-08-25 when a multi-value continuation invocation
;; stopped raising a wrong-arity error at the call site and started escaping.
;; That program is **not** what is asserted here: delivering two values to a
;; single-value context is unspecified in R7RS, and the references split on it
;; (chibi and our VM let `+` raise on the `#<values>` object; Gauche delivers
;; the first value and answers 2). The escape below is single-valued and the
;; error after it is unambiguous, so every implementation must answer `caught`.
(test-equal "an error after a continuation escape is catchable" 'caught
  (guard (e (#t 'caught))
    (begin (call-with-current-continuation (lambda (k) (k 1)))
           (car 7))))

;; The escape path's broadest effect, and the one nothing else covers: after
;; an inner `guard` fires, a *later* raise must still find the outer handler.
;;
;; The tree-walker reset `exception_handlers` to empty on every re-entry, and
;; a `guard` that fires re-enters — `guard` expands to `call/cc` +
;; `with-exception-handler`, and catching invokes `guard-k`. So one caught
;; exception emptied the handler stack for everything after it:
;;
;;   tree-walker, before => Error: unhandled exception: y
;;   VM, chibi, Gauche   => (outer y)
;;
;; Nothing in `nested_exception_handlers.rs` caught this: those tests nest
;; guards but never raise again *after* an inner one has fired, so they pass
;; either way. This is an ordinary shape — a loop that catches per item and
;; then fails on something else — not an exotic one.
(test-equal "a raise after an earlier guard fired still finds the outer handler"
  '(outer y)
  (guard (o (#t (list 'outer o)))
    (begin (guard (i (#t 'inner)) (raise 'x))
           (raise 'y))))

;; Re-entering a continuation captured under `with-exception-handler` keeps
;; the handler on both backends — converged 2026-09-01, closing audit B3.
;;
;; The VM always restored the stack from its `VmContinuation` snapshot. The
;; tree-walker's escape path in `cps_eval/mod.rs` reset `exception_handlers`
;; to empty, because `CpsContinuation` did not carry them; it does now, and
;; re-entry restores them like `dynamic_winds`. Kept as the regression guard.
(test-equal "a re-entered continuation keeps its exception handler"
  '(second-pass 42)
  (let ((saved #f) (entered #f))
    (define (run)
      (with-exception-handler
        (lambda (e) 42)
        (lambda ()
          (call/cc (lambda (k) (set! saved k) #f))
          (raise-continuable 'boom))))
    (let ((first (run)))
      (if entered
          (list 'second-pass first)
          (begin (set! entered #t) (saved #f))))))

;; ── dynamic-wind, re-entered ───────────────────────────────────────────────

;; Invoking a continuation captured inside its own `dynamic-wind` extent runs
;; the wind thunks once, on both backends — converged 2026-09-01.
;;
;;   (dynamic-wind in (lambda () (call/cc (lambda (k) (k #f)))) out)
;;   tree-walker, chibi, Gauche => (in out)
;;   VM                         => (in out in out)   until 2026-09-01
;;
;; R7RS §6.10 runs the thunks when the extent is actually left and re-entered;
;; invoking `k` here never leaves it. The VM's wind transition (then
;; `run_wind_transition`, now `step_wind_jump`) forced the common prefix to
;; zero on every full `call/cc` invoke, so it exited and re-entered even the
;; extents both stacks shared. It takes the common prefix now, keyed on the
;; wind record's identity, as the tree-walker always did.
;;
;; Found while taking `guard` to R7RS 7.3's expansion for Track L triage
;; families 22 and 28: that expansion leaves its body through a continuation
;; far more often, and under the old rule a `guard` inside a
;; `with-output-to-file` re-ran that form's after thunk — which closes the
;; port — so the next write failed on a port the program still held. Kept as
;; the regression guard: it was tracked in the PRD once, lost in an edit, and
;; recovered only by review, which is why it lives in a test.
(test-equal "a continuation within its own wind runs the thunks once" '(in out)
  (let ((log '()))
    (dynamic-wind (lambda () (set! log (cons 'in log)))
                  (lambda () (call/cc (lambda (k) (k #f))))
                  (lambda () (set! log (cons 'out log))))
    (reverse log)))

;; The same jump through the **value** form of `dynamic-wind`, which is a
;; different code path and the one that regressed while that fix was written.
;;
;; Head-position `dynamic-wind` compiles to `PushWind`/`PopWind`, so a
;; continuation resuming inside the body still reaches the instruction that
;; pops the record. The value form used to run its body on a nested Rust call
;; in `handle_control_primitive`, and an escape abandoned the frame that owned
;; the cleanup — it used to be safe to abandon it because a full continuation
;; invoke drained every wind record on the way past. Once the transition kept
;; the records both stacks share, that stopped being true, and the after-thunk
;; went from running at the wrong time to never running at all:
;;
;;   (define dw dynamic-wind) (dw in (lambda () (call/cc (lambda (k) (k #f)))) out)
;;   main VM              => (in out in)     the thunks of a jump that crossed nothing
;;   mid-fix VM           => (in)            after-thunk leaked entirely
;;   chibi, Gauche, now   => (in out)
;;
;; The leaked record also outlived its owner and fired at the next unrelated
;; transfer, so the second shape pins that a later `dynamic-wind` is
;; unaffected. `cargo test` was fully green with the leak present, because
;; nothing exercised the value form with an escaping body.
;;
;; The nested Rust call is gone since 2026-09-02 (issue #157): the value form
;; runs the same `PushWind`/`PopWind` sequence in a stub frame, so these four
;; shapes now go through the instructions head position uses. They stay
;; because they are the shapes that caught the leak.
(test-equal "the value form of dynamic-wind runs its after thunk once"
  '((in out) (in after-dw in2 out2) (in out) (in out))
  (let ((dw dynamic-wind))
    (define (probe run)
      (let ((log '()))
        (run (lambda (x) (set! log (cons x log))))
        (reverse log)))
    (list
      ;; escape stays inside the extent
      (probe (lambda (note)
               (dw (lambda () (note 'in))
                   (lambda () (call/cc (lambda (k) (k #f))))
                   (lambda () (note 'out)))))
      ;; the record must not survive to fire at a later, unrelated wind
      (probe (lambda (note)
               (dw (lambda () (note 'in))
                   (lambda () (call/cc (lambda (k) (k #f))))
                   (lambda () (note 'after-dw)))
               (dynamic-wind (lambda () (note 'in2))
                             (lambda () 'body)
                             (lambda () (note 'out2)))))
      ;; reached through apply, not a variable reference
      (probe (lambda (note)
               (apply dynamic-wind
                      (list (lambda () (note 'in))
                            (lambda () (call/cc (lambda (k) (k #f))))
                            (lambda () (note 'out))))))
      ;; escape that genuinely leaves the extent
      (probe (lambda (note)
               (call/cc (lambda (esc)
                 (dw (lambda () (note 'in))
                     (lambda () (esc 'gone))
                     (lambda () (note 'out))))))))))

;; A continuation escaping from an *after* thunk still runs the enclosing
;; after thunk — converged 2026-09-01 (triage family 30).
;;
;; R7RS 6.10: `dynamic-wind`'s third thunk runs whenever control leaves the
;; dynamic extent, and calling `k` from inside one is still leaving — the
;; outer wind has not finished unwinding, so its own after thunk is still
;; owed. The VM always paid it. The tree-walker ran the whole unwind on a
;; nested trampoline that a second jump escaped out of, so it stopped at the
;; inner thunk and never ran `outer-after`. It now runs each wind thunk as a
;; step of the trampoline the jump was made on, with the record already
;; popped, so the second jump starts from where the first had got to and the
;; outer thunk is still on its path.
;;
;; Found when Larceny's `base` suite began loading (families 14/15/23): it
;; was the one assertion in that suite the two backends answered differently.
;; chibi cannot arbitrate this one — re-entering `k` from an after thunk sends
;; it into an unbounded loop, so the row is skipped there rather than costing
;; the lane its timeout — but Gauche and the suite's own expectation agree
;; with this answer.
(cond-expand (chibi (test-skip 1)) (else))
(test-equal "a continuation from an after thunk still runs the outer after"
  '(from-after (outer-before inner-before body inner-after outer-after))
  (let ((trace '()))
    (define (note x) (set! trace (cons x trace)))
    (let ((result
           (call-with-current-continuation
             (lambda (k)
               (dynamic-wind
                 (lambda () (note 'outer-before))
                 (lambda ()
                   (dynamic-wind
                     (lambda () (note 'inner-before))
                     (lambda () (note 'body) (k 'from-body))
                     (lambda () (note 'inner-after) (k 'from-after))))
                 (lambda () (note 'outer-after)))))))
      (list result (reverse trace)))))

;; A continuation captured *inside* an after thunk while a jump is running it
;; resumes that thunk, and the jump then lands — converged 2026-09-02 with
;; the VM half of the `finally` rule.
;;
;;   both, Gauche => (escaped (before after))
;;   VM           => (() (before after after))   until 2026-09-02
;;
;; `return`'s continuation is the rest of the thunk and then the jump that
;; was running it. Each backend had to make that second half a resumable
;; thing before this could work: the tree-walker's `Jump` step (`wind.rs`),
;; the VM's `ResumeWindJump` stub frame (`runtime/vm_state.rs`). The VM's old
;; answer is what a nested Rust call gives you — the continuation captured
;; the *enclosing* frame instead, parked inside the inlined `dynamic-wind`
;; sequence at its `PopWind`, so re-entering it ran `Call after` a second
;; time and the escape value never arrived.
;;
;; Until PR #152's fix in the tree-walker's escape arm (`cps_eval/mod.rs`),
;; this program *crashed* the tree-walker with `Error: Continuation escape`:
;; the parked escape's resumption was invoked in place, and its own parked
;; escape was carried out of the trampoline by a `?`. No primitive callback
;; is needed to reach it: a wind thunk whose tail is any `call/cc` that is
;; later invoked does.
(test-equal "a continuation captured inside a running after thunk resumes it"
  '(escaped (before after))
  (let ((log '()))
    (define (note x) (set! log (cons x log)))
    (let ((r (call/cc (lambda (k)
               (dynamic-wind
                 (lambda () (note 'before))
                 (lambda () (k 'escaped))
                 (lambda ()
                   (call/cc (lambda (return)
                     (note 'after)
                     (return 'stopped)
                     (note 'unreached)))))))))
      (list r (reverse log)))))

;; A before thunk run by a re-entry sees the handlers of its own
;; `dynamic-wind` call, not those installed at the *jump* — converged
;; 2026-09-02 with the VM half of the `finally` rule.
;;
;;   both, Gauche => (outer b)
;;   VM           => (inner b)   until 2026-09-02
;;
;; The inner `guard` was not installed when `dynamic-wind` was called, so
;; R7RS 6.10 puts the before thunk's `raise` outside it. This is the
;; complement of `wind-thunk-exceptions.scm`, whose rows all have a handler
;; *missing* at the jump; here one is *extra*, and the VM's old "handlers from
;; the machine, not the record" answered wrong in that direction too. Both
;; backends now take the thunk's handler stack from the wind record.
(test-equal "a before thunk on re-entry sees its own dynamic-wind's handlers"
  '(outer b)
  (let ((k #f) (n 0))
    (let ((r (guard (o (#t (list 'outer o)))
               (dynamic-wind
                 (lambda () (set! n (+ n 1)) (if (= n 2) (raise 'b)))
                 (lambda () (call/cc (lambda (c) (set! k c) 'first)))
                 (lambda () #f)))))
      (if (eq? r 'first)
          (guard (i (#t (list 'inner i))) (k 'second))
          r))))

;; A continuation re-entering the body of the **value** form of
;; `dynamic-wind` finds the call still intact — converged 2026-09-02 (issue
;; #157).
;;
;; Two symptoms, one cause. The call's remaining obligations — deliver the
;; body's value, pop the record, run *its own* after-thunk — used to live in
;; the Rust frame `handle_control_primitive` ran the body on, and a re-entry
;; restores the VM's frames, not that one:
;;
;;   (define r (dw (lambda () #f) «capture k, return 'first» (lambda () #f)))
;;   (if (eq? r 'first) (k 'second) #f)          => second, VM said ()
;;
;;   (dw in1 «capture saved» out1) then (dw in2 (lambda () (saved 'second)) out2)
;;                                              => (in1 out1 in2 out2 in1 out1),
;;                                                 VM said (… in1 out2)
;;
;; The `()` was the `NULL` `call/cc`'s capture cleared `dst` to, left in a
;; live register — downstream it surfaced as an unrelated `type error:
;; expected a procedure, got null` rather than as a visibly wrong value. The
;; wrong after-thunk came from the `Escaped` arm deciding what it still owed
;; with a *length* test, `dynamic_winds.len() > wind_depth`, applied after
;; the jump had already replaced that stack with the target's: it truncated
;; the target's records and re-ran its own.
;;
;; The fix is the move PR #156 made for a jump's wind thunks — the value form
;; now runs the same `PushWind`/`Call`/`PopWind` sequence head position
;; compiles to, in a stub frame of its own, so "the rest of the
;; `dynamic-wind`" is a pc that the continuation restores. Head position was
;; never affected, for exactly that reason.
(test-equal "the value form of dynamic-wind survives a re-entry into its body"
  'second
  (let ((dw dynamic-wind) (k #f))
    (let ((r (dw (lambda () #f)
                 (lambda () (call/cc (lambda (c) (set! k c) 'first)))
                 (lambda () #f))))
      (if (eq? r 'first) (k 'second) #f)
      r)))

;; Re-entering extent 1 from inside extent 2 leaves extent 2 (`out2`) and
;; enters extent 1 (`in1`); when the resumed body returns, extent 1 closes
;; with its *own* after-thunk, `out1`.
(test-equal "and the re-entered value form closes with its own after thunk"
  '(in1 out1 in2 out2 in1 out1)
  (let ((dw dynamic-wind) (log '()) (saved #f) (done #f))
    (define (note x) (set! log (cons x log)))
    (dw (lambda () (note 'in1))
        (lambda () (call/cc (lambda (c) (set! saved c) 'first)))
        (lambda () (note 'out1)))
    (if (not done)
        (begin (set! done #t)
               (dw (lambda () (note 'in2))
                   (lambda () (saved 'second))
                   (lambda () (note 'out2)))))
    (reverse log)))

;; A continuation captured in the value form's **before** or **after** thunk
;; and re-entered after the call has returned — issue #159, converged
;; 2026-09-02 with the body case above and by the same change.
;;
;; The value form ran each thunk on a nested dispatch loop. While that loop is
;; still on the Rust stack a continuation captured in the thunk resumes fine —
;; a retry loop inside a before-thunk always worked, on `main` too. It is the
;; *late* re-entry, after the `dynamic-wind` call has returned and the loop is
;; gone, that had nothing to come back to:
;;
;;   A: capture in `before`, re-enter later => (val (in body out body out))
;;   B: capture in `after`,  re-enter later => (val (in body out))
;;   main VM said (#<unknown> (in body out)) to both
;;
;; `#<unknown>` is an uninitialised register reaching user-visible output: the
;; re-entry delivered into a frame that no longer existed, and the rest of the
;; `dynamic-wind` — the body, the after-thunk, the value — never ran at all.
;; The tree-walker, Gauche and chibi all give the two answers above.
;;
;; `probe` sequences the run and the log read with `let*` rather than writing
;; `(list (run …) (reverse log))`. R7RS leaves argument order unspecified and
;; chibi evaluates right-to-left, so the shorter spelling reads an empty log
;; there and answers `(val ())` to both rows — a bug in the *test*, not a
;; disagreement about `dynamic-wind`. It was written that way first, and
;; cross-checking the row against chibi is what caught it.
(test-equal "the value form of dynamic-wind re-enters its before and after thunks"
  '((val (in body out body out)) (val (in body out)))
  (let ((dw dynamic-wind))
    (define (probe run)
      (let ((log '()))
        (let* ((r (run (lambda (x) (set! log (cons x log)))))
               (l (reverse log)))
          (list r l))))
    (list
      ;; A — the resumed before-thunk returns, and the rest of the call runs
      ;; a second time from there.
      (probe (lambda (note)
               (let ((k #f) (done #f))
                 (let ((r (dw (lambda () (note 'in) (call/cc (lambda (c) (set! k c))))
                              (lambda () (note 'body) 'val)
                              (lambda () (note 'out)))))
                   (if (not done) (begin (set! done #t) (k #f)))
                   r))))
      ;; B — the resumed after-thunk returns, and the call is then over, so
      ;; nothing repeats.
      (probe (lambda (note)
               (let ((k #f) (done #f))
                 (let ((r (dw (lambda () (note 'in))
                              (lambda () (note 'body) 'val)
                              (lambda () (note 'out) (call/cc (lambda (c) (set! k c)))))))
                   (if (not done) (begin (set! done #t) (k #f)))
                   r)))))))

;; A `call/cc` retry loop *inside* one of the value form's wind thunks, which
;; resumes while the thunk is still running.
;;
;; Not a converged row — `main` answered this correctly too, because the
;; nested dispatch loop the thunk ran on was still on the Rust stack to resume
;; into. It is the *late* re-entry, after that loop is gone, that was broken
;; (#159, the row above). It is here as a guard on the rewritten path: the
;; thunks are ordinary frames of `value_wind_stub` now, and this is the shape
;; that would notice if the stub's register window or its `Call` sequence got
;; the thunk's own re-entry wrong.
(test-equal "the value form of dynamic-wind captures inside its own thunks"
  '((0 1 2 body out) (in body 0 1 2))
  (let ((dw dynamic-wind))
    (define (probe run)
      (let ((log '()))
        (run (lambda (x) (set! log (cons x log))))
        (reverse log)))
    (list
      ;; a retry loop inside the before-thunk
      (probe (lambda (note)
               (let ((n 0))
                 (dw (lambda ()
                       (let ((k (call/cc (lambda (c) c))))
                         (note n)
                         (set! n (+ n 1))
                         (if (< n 3) (k k))))
                     (lambda () (note 'body))
                     (lambda () (note 'out))))))
      ;; and one inside the after-thunk
      (probe (lambda (note)
               (let ((n 0))
                 (dw (lambda () (note 'in))
                     (lambda () (note 'body))
                     (lambda ()
                       (let ((k (call/cc (lambda (c) c))))
                         (note n)
                         (set! n (+ n 1))
                         (if (< n 3) (k k)))))))))))

;; ── guard, after the unwind and around a declined raise ────────────────────

;; A `guard` clause runs after the unwind, on both backends — converged
;; 2026-09-01 with Track L triage families 22 and 28.
;;
;;   VM, chibi, Gauche => (before after handler)
;;   tree-walker       => (before handler after)   until 2026-09-01
;;
;; R7RS §4.2.7 evaluates the clauses in the `guard`'s own dynamic environment,
;; so the after-thunk runs before them. Not cosmetic: a handler writing to
;; `current-output-port` wrote into whatever the un-unwound extent installed,
;; which is how it was found.
;;
;; The tree-walker diverged because `(error "x")` reached the handler from
;; `apply_error`, which — alone among the three raise paths — did not unwind
;; first. The fix took the *other* two down to `apply_error`'s behaviour
;; rather than the reverse: no raise path unwinds now, and the unwind comes
;; from `guard-k`, which is where R7RS puts it. See
;; `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6.
(test-equal "a guard clause runs after the unwind" '(before after handler)
  (let ((log '()))
    (guard (e (#t (set! log (cons 'handler log))))
      (dynamic-wind (lambda () (set! log (cons 'before log)))
                    (lambda () (error "x"))
                    (lambda () (set! log (cons 'after log)))))
    (reverse log)))

;; A `guard` survives one of its clauses declining a `raise-continuable`.
;;
;; The `guard`'s handler declines `'x`, which re-raises it through `handler-k`
;; to the outer handler; that returns `(I x)`, and the body continues to raise
;; `'y` — which the `guard` must catch. chibi and Gauche agree.
;;
;; The VM used to answer `((I x) (I y))`: its continuable path popped the
;; handler to run it and re-pushed it only when the handler *returned*, in
;; Rust after a nested dispatch loop, and `guard`'s handler leaves through
;; `handler-k` instead — so the re-push was skipped and the `guard` was
;; silently uninstalled for the rest of its body. Converged 2026-09-05 with
;; issue #178, which made the re-push an instruction in a frame: `handler-k`
;; captures that frame like any other, so re-entering it runs the re-push.
(test-equal "a guard survives declining a continuable raise" 'caught-y
  (with-exception-handler (lambda (e) (list 'I e))
    (lambda () (guard (e ((eq? e 'y) 'caught-y))
      (let* ((x (raise-continuable 'x))
             (y (raise-continuable 'y)))
        (list x y))))))

;; ── Raise paths: what reaches the handler, and from where ──────────────────

;; Bad syntax handed to the `eval` primitive is the *caller's* error, raised
;; while the program runs — catchable, on both backends. The tree-walker used
;; to wrap it in a non-catchable `InternalError` (so this program died) while
;; the VM caught it; converged when the D3 error-class work relabeled the
;; eval-primitive path as `InvalidSyntax`. `EvalError::DesugarError` stays
;; reserved for the `Backend::eval` entry, where nothing is running yet.
(test-equal "bad syntax handed to eval is catchable" 'caught
  (guard (e (#t 'caught)) (eval '(if) (interaction-environment))))

;; Converged 2026-08-15: an unbound variable is a catchable condition in every
;; position, on both backends.
;;
;; The tree-walker's CPS step function routed lookup failures through the
;; Scheme exception handlers in some arms and `?`-propagated them in others,
;; so whether `guard` caught the error depended on where the variable sat.
;; chibi, Gauche and Chez catch every position here; the VM already did.
;; Enforced structurally by the `try_catchable!` macro in `step.rs`; history
;; in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6. One row per position, so a
;; regression names the arm.
(test-equal "an unbound variable is catchable: bare reference" 'caught
  (guard (e (#t 'caught)) undefined-name))
(test-equal "an unbound variable is catchable: operator position" 'caught
  (guard (e (#t 'caught)) (undefined-name)))
(test-equal "an unbound variable is catchable: operand position" 'caught
  (guard (e (#t 'caught)) (list (undefined-name))))
(test-equal "an unbound variable is catchable: operand of a primitive" 'caught
  (guard (e (#t 'caught)) (+ 1 (undefined-name))))
(test-equal "an unbound variable is catchable: if test" 'caught
  (guard (e (#t 'caught)) (if undefined-name 1 2)))
(test-equal "an unbound variable is catchable: set! target" 'caught
  (guard (e (#t 'caught)) (set! undefined-name 1)))
(test-equal "an unbound variable is catchable: define value" 'caught
  (guard (e (#t 'caught)) (define x undefined-name) x))
(test-equal "an unbound variable is catchable: call/cc operand" 'caught
  (guard (e (#t 'caught)) (call/cc undefined-name)))
(test-equal "an unbound variable is catchable: unquote" 'caught
  (guard (e (#t 'caught)) `(,undefined-name)))

;; A handler that returns from a non-continuable `raise` raises the secondary
;; exception R7RS 6.11 asks for.
;;
;; "If the handler returns, a secondary exception is raised in the same
;; dynamic environment as the handler." The VM used to deliver the handler's
;; value to the raise's destination register as if the raise had been
;; continuable — `(returned)` for the first row, and for the second, where
;; the raise is a *primitive's* error routed with register 0 as its
;; destination, the returning handler's value landed in r0 and `car`'s own
;; destination was left holding `()`. It could not tell a handler that
;; returned from one that escaped, because both came back through the same
;; nested run loop.
;;
;; Converged 2026-09-05 with issue #178: the return lands on `ResumeRaise`,
;; an instruction, which sees it whatever the handler was and whichever route
;; the raise took.
;;
;; Two rows per thunk. The portable one asserts what R7RS says: the outer
;; `guard` receives an *error object* — neither the handler's `'returned`
;; delivered as a value nor the original `'x` re-raised bare. That cannot
;; distinguish the secondary from the primary when the primary is itself an
;; error object, as `car`'s is, so the Patina-scoped one pins the wording the
;; `.rs` row pinned, "exception handler returned from non-continuable
;; exception" — not portable, which is why it is scoped rather than dropped.
(test-equal "a handler returning from a non-continuable raise raises the secondary"
  '(outer #t)
  (guard (o (#t (list 'outer (error-object? o))))
    (with-exception-handler (lambda (e) 'returned)
      (lambda () (list (raise 'x))))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "and it is the secondary, not the primary, that arrives"
  '(outer "exception handler returned from non-continuable exception")
  (guard (o (#t (list 'outer (error-object-message o))))
    (with-exception-handler (lambda (e) 'returned)
      (lambda () (list (raise 'x))))))

(test-equal "and from a primitive's error, where the raise has no source form"
  '(outer #t)
  (guard (o (#t (list 'outer (error-object? o))))
    (with-exception-handler (lambda (e) 'returned)
      (lambda () (list (car 5))))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "and a primitive's error is replaced by the secondary too"
  '(outer "exception handler returned from non-continuable exception")
  (guard (o (#t (list 'outer (error-object-message o))))
    (with-exception-handler (lambda (e) 'returned)
      (lambda () (list (car 5))))))

;; A continuation used *as* the handler, for a primitive's error.
;;
;; `(call/cc (lambda (k) (with-exception-handler k thunk)))` is R7RS's own
;; idiom for capturing a raised object; chibi and Gauche answer #t too.
;; Triage family 24 made it work for `raise` on the VM, but a `VmError` from a
;; primitive takes the run loop's route into `vm_raise_value`, which called
;; the handler through `call_any` — the narrow dispatcher, which does not
;; accept a continuation. Converged 2026-09-05 with issue #178: the handler
;; is called by the `Call` instruction of `raise_step_stub` now, which is the
;; same dispatcher every other call goes through.
(test-equal "a continuation can be the handler for a primitive error" #t
  (error-object? (call/cc (lambda (k) (with-exception-handler k (lambda () (car 5)))))))

;; ── Backtracking, and re-entering nested extents ───────────────────────────
;;
;; Moved from `crates/patina-tests/tests/cps_features.rs` (#193 Phase 2) with
;; its prompt half, which is `prompts.scm` now; these two use only `call/cc`
;; and `dynamic-wind`, so they are portable and live here, where the oracles
;; answer them.

;; Classic amb-style backtracking (from chibi's `test08-callcc.scm`): the
;; first Pythagorean triple with every side in 2..9, encoded as x*100+y*10+z.
;; Which one comes first depends on the order `let` evaluates its inits,
;; which R7RS leaves unspecified — 534 left to right, 543 right to left — so
;; the row accepts either and checks that the answer really is a triple.
(test-assert "amb-style backtracking finds a Pythagorean triple"
  (let ()
    (define fail (lambda () 999999))
    (define (enumerate a b cont)
      (if (< b a)
          (fail)
          (let ((save fail))
            (set! fail (lambda () (set! fail save) (enumerate (+ a 1) b cont)))
            (cont a))))
    (define (in-range a b)
      (call-with-current-continuation (lambda (cont) (enumerate a b cont))))
    (let ((result (let ((x (in-range 2 9))
                        (y (in-range 2 9))
                        (z (in-range 2 9)))
                    (if (= (* x x) (+ (* y y) (* z z)))
                        (+ (* x 100) (+ (* y 10) z))
                        (fail)))))
      (and (memv result '(534 543))
           (let ((x (quotient result 100))
                 (y (remainder (quotient result 10) 10))
                 (z (remainder result 10)))
             (= (* x x) (+ (* y y) (* z z))))))))

;; Re-entering a continuation captured inside two nested extents runs both
;; before thunks, once each, and leaves the extents standing. The value form
;; of `dynamic-wind` is what made this a real test: its records used to carry
;; the frame depth of the call, and a jump that *enters* an extent pushed a
;; record whose depth belonged to another stack — captured deep, re-entered
;; from much shallower, it looked like the body returning, and the VM ran
;; `out-a` under the still-running entry and went round for ever. Neither
;; half of that exists now (issue #157); the shape stays because re-entry
;; across two nested extents, from a shallower stack, is worth holding both
;; backends to whatever the mechanism. `note` raises past a bounded log, so a
;; regression fails the row rather than spinning.
(test-equal "re-entering nested value-form extents runs each thunk once"
  '(in-a in-b out-b out-a in-a in-b out-b out-a)
  (let ((k #f) (log '()) (dw dynamic-wind))
    (define (note x)
      (if (> (length log) 12) (error "wind thunks are looping"))
      (set! log (cons x log)))
    (define (deep n)
      (if (= n 0)
          (dw (lambda () (note 'in-a))
              (lambda ()
                (dw (lambda () (note 'in-b))
                    (lambda () (call/cc (lambda (c) (set! k c) 'first)))
                    (lambda () (note 'out-b))))
              (lambda () (note 'out-a)))
          (car (list (deep (- n 1))))))
    (let ((first-time? (eq? 'first (deep 6))))
      (if first-time? (k 'second))
      (reverse log))))

;; ── A primitive's callback ─────────────────────────────────────────────────
;;
;; A Rust primitive's callback — `member` or `assoc` with a predicate,
;; `call-with-port`, `force`, a parameter converter — runs on a nested
;; trampoline on the tree-walker. Until 2026-09-10 that trampoline started with
;; every stack empty and read every continuation invoke inside the callback as
;; leaving the primitive, so: a `raise` in the callback found no handler, a
;; retry loop or a local `call/cc` inside the callback abandoned the primitive
;; and ran the rest of the *program* from inside it, and an abort from the
;; callback found no prompt. Inside a suite file "the rest of the program" was
;; every later row, so four of these were `assert_divergence` quarantines in
;; `escape_from_primitive.rs` — they vanished as rows rather than failing.
;; Each trampoline now inherits its caller's stacks and knows which
;; continuations end in it (`cps_eval/types.rs`, `callback.rs`), and every row
;; here is an ordinary both-backend assertion. `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md`
;; §6 has the history under "primitive's callback".

;; A `call/cc` retry loop inside a `call-with-port` callback. Two things are
;; asserted: the value, "012", which says the port stayed open across the
;; re-entries (R7RS 6.13.1 closes it only "if `proc` returns"; until
;; 2026-09-01 the tree-walker failed here with `I/O error: port is closed`),
;; and `after`, which says the rest of the program ran once, after the
;; primitive returned — the half a one-expression program could not see.
(test-equal "call-with-port survives an in-extent continuation invoke"
  '("012" (after))
  (let ((log '()))
    (let* ((r (call-with-port (open-output-string)
                (lambda (p)
                  (let ((n 0))
                    (let ((k (call/cc (lambda (c) c))))
                      (write-string (number->string n) p)
                      (set! n (+ n 1))
                      (if (< n 3) (k k)))
                    (get-output-string p)))))
           (l (begin (set! log (cons 'after log)) log)))
      (list r l))))

;; A callback that captures and invokes its *own* continuation, returning
;; normally, is not an escape: the primitive runs to completion. The
;; tree-walker used to answer #f — the callback's value — and, followed by
;; one more form, reached it with the `define` still unbound.
(test-equal "a callback using its own continuation returns the primitive's value"
  '((2 3) (after))
  (let ((log '()))
    (let* ((r (member 2 '(1 2 3) (lambda (a b) (call/cc (lambda (k2) (k2 (= a b)))))))
           (l (begin (set! log (cons 'after log)) log)))
      (list r l))))

;; A declining `guard` clause inside the callback. R7RS 7.3's `guard`
;; re-raises a declined condition by jumping back *into* the raise point
;; through `handler-k` and calling `raise-continuable` there, so the next
;; handler out is the one installed around the raise — which on the
;; tree-walker's old empty-stacked trampoline was nothing, and the raw symbol
;; escaped every handler in the program. Both raise forms.
(test-equal "a declining guard inside a port callback reaches the outer guard: raise"
  '(outer sym)
  (guard (outer (#t (list 'outer outer)))
    (call-with-port (open-input-string "a")
      (lambda (p) (guard (e ((string? e) 'no)) (raise 'sym))))))

(test-equal "a declining guard inside a port callback reaches the outer guard: raise-continuable"
  '(outer sym)
  (guard (outer (#t (list 'outer outer)))
    (call-with-port (open-input-string "a")
      (lambda (p) (guard (e ((string? e) 'no)) (raise-continuable 'sym))))))

;; A declining `guard` *outside* the callback. The raise inside the callback
;; finds the inner guard's handler (inherited), whose escape unwinds the
;; callback's trampoline; the clause declines and re-raises through
;; `handler-k`, a continuation captured *in* that trampoline — which has
;; returned by then. A run that has returned is resumed by the nearest
;; enclosing form-level run, so the re-raise lands at the raise point with
;; the outer handler next, as R7RS 7.3 asks. The first cut of the fix made
;; this an error, and the review caught it: it was `(outer #<error …>)` on
;; the old tree-walker — wrong object, but caught — and must not get worse.
(test-equal "a declining guard outside a port callback reaches the outer guard"
  '(outer sym)
  (guard (outer (#t (list 'outer outer)))
    (guard (e ((string? e) 'no))
      (call-with-port (open-input-string "a") (lambda (p) (raise 'sym))))))

;; A handler that returns from a non-continuable raise inside a callback is
;; called **once**. The callback's run pops the handler and calls it; when it
;; returns, the secondary exception must see only the handlers *outside* it
;; (R7RS 6.11). The first cut of the fix routed the escaping secondary
;; through the calling step's handler stack — the same handlers the callback
;; had inherited — and the handler ran a second time with the secondary.
(test-equal "a handler returning inside a callback is called once: raise"
  '(outer 1 #t)
  (let ((n 0))
    (guard (o (#t (list 'outer n (error-object? o))))
      (with-exception-handler (lambda (e) (set! n (+ n 1)) 'ignored)
        (lambda () (member 1 '(1 2) (lambda (a b) (raise 'x))))))))

(test-equal "a handler returning inside a callback is called once: error"
  '(outer 1)
  (let ((n 0))
    (guard (o (#t (list 'outer n)))
      (with-exception-handler (lambda (e) (set! n (+ n 1)) 'ignored)
        (lambda () (member 1 '(1 2) (lambda (a b) (error "boom"))))))))

;; A continuation captured inside an after-thunk that is running as a step
;; of a jump *out of* the callback. The thunk runs on the callback's
;; trampoline, so the capture is stamped with it, but its chain ends in the
;; jump's target, in the form outside. Re-entering it after the callback has
;; returned runs the rest of the thunk and then completes the jump: `r` is
;; `escaped` a second time, and the thunk's own log line is written twice.
(test-equal "a capture inside an after thunk during a jump out of a callback re-enters"
  '(escaped 2 (after after))
  (let ((n 0) (saved #f) (log '()))
    (let ((r (call/cc (lambda (out)
               (member 1 '(1) (lambda (a b)
                 (dynamic-wind (lambda () #f)
                               (lambda () (out 'escaped))
                               (lambda () (call/cc (lambda (k) (set! saved k)))
                                          (set! log (cons 'after log))))))))))
      (set! n (+ n 1))
      (if (= n 1) (saved 'again))
      (list r n (reverse log)))))

;; An unquote runs under the enclosing dynamic environment. The tree-walker
;; evaluates `,expr` on a nested form run, like the `eval` primitive, and
;; until 2026-09-10 that run started with no handlers — the same shape as
;; the callback defect, in a place the first cut of the fix missed.
(test-equal "an unquote sees the enclosing guard" '(sym y)
  (guard (e ((symbol? e) (list 'sym e)) (#t (list 'other (error-object? e))))
    `(1 ,(raise 'y))))

(test-equal "an unquote sees the enclosing handler" '(1 10)
  (with-exception-handler (lambda (e) 10) (lambda () `(1 ,(raise-continuable 'x)))))

(test-equal "an unquote can escape" 2
  (call/cc (lambda (k) `(1 ,(k 2)))))

;; ── Escaping out of a callback ──────────────────────────────────────────────
;;
;; Moved from `escape_from_primitive.rs` once the trampoline fix made them
;; portable; what stays there needs a file on disk or `eval`'s environment.
;; On the VM these were the escape whose result used to be written through a
;; register base of a frame that no longer existed (`index out of bounds` in
;; `set_reg_at`, until 2026-08-15); the primitive is *abandoned* now rather
;; than left running on a stack it no longer owns.

;; The bad register offset tracked frame depth rather than being a fixed
;; mistake, so escaping twice and from a nested depth is the case that would
;; catch an off-by-one "fix" working at one depth only.
(test-equal "an escape out of a callback, repeatedly and from a nested depth"
  '(deep deep deep)
  (let ()
    (define (run) (call/cc (lambda (k) (member 2 '(1 2 3) (lambda (a b) (k 'deep))))))
    (define (nested) (call/cc (lambda (k) (member 2 '(1 2) (lambda (a b) (k (run)))))))
    (let* ((a (run)) (b (run)) (c (nested)))
      (list a b c))))

;; A closure comparator that does *not* escape must still work — the VM's
;; guard fires on frame depth, and one that fired spuriously would break
;; every re-entrant call. These are the closure forms, the ones that push a
;; frame; the primitive-comparator forms are covered elsewhere.
(test-equal "a closure callback that does not escape still works"
  '((2 3) (2 . b))
  (list (member 2 '(1 2 3) (lambda (a b) (= a b)))
        (assoc 2 '((1 . a) (2 . b)) (lambda (a b) (= a b)))))

;; Reaching the same primitive other than by call position: `apply` and
;; value-position dispatch go through a different path on the VM, which has
;; no depth check of its own — they work because the escape is signalled
;; from the re-entry boundary, and every route unwinds the same way.
(test-equal "an escape out of a callback reached through apply" 'x
  (call/cc (lambda (k) (apply member (list 2 '(1 2 3) (lambda (a b) (k 'x)))))))

(test-equal "an escape out of a callback reached in value position" 'x
  (call/cc (lambda (k)
    (let ((ops (list member))) ((car ops) 2 '(1 2 3) (lambda (a b) (k 'x)))))))

;; The primitive stops when the continuation is invoked, instead of running
;; on to completion. `member` would otherwise keep calling the comparator for
;; the remaining elements — each call re-invoking the continuation.
(test-equal "the escaped-from primitive is abandoned" '(#f (1))
  (let ((seen '()))
    (let ((r (call/cc (lambda (k)
               (member 9 '(1 2 3)
                 (lambda (a b) (set! seen (cons b seen)) (k #f)))))))
      (list r (reverse seen)))))

;; The parameter *set* path, which runs a converter through a different
;; boundary than `make-parameter` construction does. On the VM this used to
;; lose the enclosing top-level `define` outright.
(test-equal "an escape out of a parameter converter during parameterize"
  'from-converter
  (let ((kk #f))
    (let ((p (make-parameter 0 (lambda (v) (if kk (kk 'from-converter) v)))))
      (call/cc (lambda (k) (set! kk k) (parameterize ((p 1)) 'done))))))

;; A raise inside the callback reaches the outer `guard` as the object that
;; was raised. The tree-walker used to report the callback's unhandled raise
;; as an *error*, which the outer trampoline then routed to the `guard` as an
;; error object whose message was `unhandled exception: x`; a clause testing
;; for `'x` declined, and the program died re-raising an object nobody
;; raised. This row was the one of the family that could be a scoped
;; expectation while the rest were Rust: its wrong answer was at least
;; delivered to the row.
(test-equal "a raise inside a port callback reaches the guard as the raised object"
  '(sym x)
  (guard (e ((symbol? e) (list 'sym e))
            ((error-object? e) (raise (error-object-message e))))
    (call-with-port (open-input-string "a") (lambda (p) (raise 'x)))))

(test-end)
