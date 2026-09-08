;; CPS-dependent control flow — exception handler stacks, nested `dynamic-wind`,
;; continuation capture, `guard`, and the routing of runtime errors.
;;
;; Migrated from `crates/patina-tests/tests/cps_features.rs` (#193 Phase 1),
;; which is being split rather than moved whole. That file had three parts with
;; different portability:
;;
;;   - **this file**, 51 rows: 46 of plain R7RS control flow, plus Larceny
;;     family 27's four and one continuation-as-handler row, all moved in from
;;     `larceny_families.rs`;
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

(import (scheme base) (scheme write) (scheme file) (scheme read) (srfi 64))

;; `(scheme stream)` is imported only where it is used. It backs one row, that
;; row is scoped to Patina, and it is R7RS-large Red rather than R7RS-small —
;; so importing it unconditionally would put all 49 other rows at the mercy of
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

;; The `.rs` row compared "1\n2\n3", which was the harness rendering a values
;; object rather than anything a program can see. `call-with-values` is how a
;; program asks.
(test-equal "a continuation can be handed multiple values" '(1 2 3)
  (call-with-values
    (lambda () (call-with-current-continuation (lambda (k) (k (values 1 2 3)))))
    list))

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

(test-end)
