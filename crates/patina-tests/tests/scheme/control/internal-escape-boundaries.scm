;; Continuations escaping past the VM's *internal* synchronous boundaries.
;;
;; Sibling of `escape_from_primitive.rs`, which covers the four `ApplyContext`
;; boundaries. Those were made correct in 2026-08-15 and stayed correct; the
;; boundaries here — the `run_thunk` calls the VM makes to itself — were not,
;; and #87 (control primitives callable as values) put user code on the far
;; side of them. Audit 2026-08-17, group A and C3.
;;
;; Migrated whole from `crates/patina-tests/tests/internal_escape_boundaries.rs`
;; (#193 Phase 1). Nothing had to stay behind: every row was one
;; `assert_program_eval_to`, which is a claim about the language.
;;
;; The tree-walker answered all of these correctly before the fix, so it is the
;; expectation. Gauche agrees with all 11 rows. **chibi agrees with 10 of them**
;; and dies with `out of stack space` on the last, which is why that row is
;; ordered last — see the note above it.
;;
;; ## The sweep (audit item A4)
;;
;; Every place the VM re-enters its own dispatch loop, and what guards it:
;;
;;   dynamic-wind's three thunks (value form)
;;       *not a boundary any more* — since 2026-09-02 all three are ordinary
;;       frames of `value_wind_stub`, the same PushWind/Call/PopWind sequence
;;       head position compiles to (issue #157)
;;   call-with-values producer (value form)
;;       `run_thunk_outcome` — skips the consumer
;;   `vm_raise_value` continuable handler
;;       `run_loop_until_outcome` — no re-push, no `set_reg`
;;   a jump's exit / enter thunks (`step_wind_jump`)
;;       *not a boundary any more* — since 2026-09-02 each thunk is an ordinary
;;       frame under a `ResumeWindJump` stub, so an escape out of one is an
;;       ordinary escape and a continuation captured in one is resumable
;;   `AbortCurrentContinuation` exit winds — both the control primitive and
;;   `Instruction::Abort`, one boundary with two call sites
;;       `run_thunk` — pops before running
;;   `try_invoke_continuation` delimited enter thunks    `run_thunk`
;;   `Instruction::InvokeContinuation` enter thunks      `run_thunk`
;;
;; The decision itself lives in exactly one place: `run_loop_until_outcome`
;; compares the restored frame depth against its own `exit_depth`. Boundaries do
;; not each re-derive it — the two that did (`call_value_with_probe` and its
;; tail twin) got the `==` case wrong, which is how an escape into the caller's
;; own frame both clobbered a register and abandoned the rest of the form.

(import (scheme base) (srfi 64))

(test-begin "internal-escape-boundaries")

;; ── Escapes out of the value forms ──────────────────────────────────────────

;; A2 — `dynamic-wind` called as a value, escaped out of from its body.
;;
;; The fifteen live locals are not decoration: in the `.rs` form this row came
;; from, they made the register file wide enough that the write through the dead
;; frame landed out of bounds, so before the fix this was a process abort in
;; `set_reg_at` rather than a wrong answer.
;;
;; **That history is not what this row now pins.** Inside `test-equal` the
;; expression runs nested in SRFI 64's own `call/cc` and handler, at a different
;; frame depth from the top-level program it used to be, and the abort was never
;; something a green run could demonstrate anyway. What is pinned is the answer:
;; the escape delivers 7. Reproducing the original abort needs the pre-fix VM.
(define dw dynamic-wind)

(define (escape-from-a-wide-frame k)
  (let ((x1 1) (x2 2) (x3 3) (x4 4) (x5 5) (x6 6) (x7 7) (x8 8)
        (x9 9) (x10 10) (x11 11) (x12 12) (x13 13) (x14 14) (x15 15))
    (let ((r (dw (lambda () 0) (lambda () (k 7) 99) (lambda () 0))))
      (+ r x1 x2 x3 x4 x5 x6 x7 x8 x9 x10 x11 x12 x13 x14 x15))))

(test-equal "escape from a dynamic-wind body called as a value" 7
  (call/cc (lambda (k) (escape-from-a-wide-frame k) 999)))

;; The other half of A2: the escape must also *resume* the form it returns into.
;; Reporting the escape as a normal return unwound every enclosing dispatch
;; loop, including the one that still owned the restored frame, so the rest of
;; the `let` never ran.
(test-equal "the form the escape returns into still runs" '(resumed 7)
  (let ((r (call/cc (lambda (k)
             (dw (lambda () 0) (lambda () (k 7) 99) (lambda () 0))
             999))))
    (list 'resumed r)))

(test-equal "escape from the before thunk called as a value" 'from-before
  (call/cc (lambda (k)
    (dw (lambda () (k 'from-before)) (lambda () 'body) (lambda () 'after)))))

(test-equal "escape from the after thunk called as a value" 'from-after
  (call/cc (lambda (k)
    (dw (lambda () 0) (lambda () 'body) (lambda () (k 'from-after))))))

;; An escape out of the value form still runs the after-thunk exactly once — the
;; wind transition the continuation performs is what runs it, which is why the
;; abandoned call must *not* run its own cleanup as well.
(define once-log '())
(test-equal "the after thunk runs once on escape" '(escaped (in out))
  (let ((r (call/cc (lambda (k)
             (dw (lambda () (set! once-log (cons 'in once-log)))
                 (lambda () (k 'escaped))
                 (lambda () (set! once-log (cons 'out once-log))))))))
    (list r (reverse once-log))))

;; C3 — `call-with-values` as a value, producer escapes. The consumer ran
;; anyway, on the escape value, and its result then replaced it.
(define cwv call-with-values)
(define consumer-ran 'no)
(test-equal "escape from a call-with-values producer called as a value" '(42 no)
  (let ((r (call/cc (lambda (k)
             (cwv (lambda () (k 42))
                  (lambda vs (set! consumer-ran 'consumer-ran) 99))))))
    (list r consumer-ran)))

;; A4 — the continuable-`raise` handler path, which runs its handler
;; synchronously and then re-pushes the handler and writes a register.
(test-equal "escape from a raise-continuable handler" '(handler-escaped oops)
  (call/cc (lambda (k)
    (with-exception-handler (lambda (e) (k (list 'handler-escaped e)))
      (lambda () (+ 1 (raise-continuable 'oops)))))))

;; ── Not escapes ─────────────────────────────────────────────────────────────
;;
;; The fix turns a continuation invocation into an unwind, so these say that the
;; ordinary uses still resume rather than exit.

(define no-escape-log '())
(test-equal "the value form of dynamic-wind still works without any escape"
  '(body (in out))
  (let ((r (dw (lambda () (set! no-escape-log (cons 'in no-escape-log)))
               (lambda () 'body)
               (lambda () (set! no-escape-log (cons 'out no-escape-log))))))
    (list r (reverse no-escape-log))))

(test-equal "the value form of call-with-values still works" '(1 2)
  (cwv (lambda () (values 1 2)) list))

;; A continuation captured *inside* a wind body and re-invoked there is an
;; in-extent jump, not an escape: the loop that owns the restored frame keeps
;; dispatching.
(define retries 0)
(test-equal "a continuation used inside a wind body is not an escape" '(retried 2)
  (dynamic-wind
    (lambda () 0)
    (lambda () (let ((r (call/cc (lambda (c) c))))
                 (set! retries (+ retries 1))
                 (if (procedure? r) (r retries) (list 'retried retries))))
    (lambda () 0)))

;; ── Last, on purpose: the row chibi dies on ─────────────────────────────────

;; A3 — a continuation invoked from an after-thunk while `raise` unwinds.
;;
;; The record was still on `dynamic_winds` while its own after-thunk ran, so the
;; escape re-entered it and re-ran the same thunk, with no depth guard:
;; `run_thunk → try_invoke_continuation → run_wind_transition → run_thunk` until
;; the native stack gave out and aborted the process. Popping the record first
;; closed it; running the thunks as frames rather than nested Rust calls
;; (2026-09-02) means the recursion has nowhere to build up either.
;;
;; **chibi dies on this row** — `ERROR: out of stack space`, measured
;; 2026-09-07 — so it is last, and it is skipped there. Ordering alone was not
;; enough: SRFI 64 stops the file where the death happens, and its summary is
;; printed at `test-end`, so the oracle lane saw no output at all and the file
;; was registered as one chibi cannot run. `test-skip` prevents *evaluation*,
;; so with the row skipped chibi reaches `test-end` and reports — 10 pass, 1
;; skip, measured 2026-09-09. That is the difference between chibi
;; corroborating ten rows in a log nobody reads and corroborating them in the
;; lane. Keep the row here, and put any future row chibi cannot survive beside
;; it, with its own skip.
(cond-expand (chibi (test-skip 1)) (else))
(test-equal "escape from an after thunk during raise unwinding" 'escaped-from-after
  (call/cc (lambda (k)
    (guard (e (#t (list 'caught e)))
      (dynamic-wind (lambda () 0)
                    (lambda () (raise 'boom))
                    (lambda () (k 'escaped-from-after)))))))

(test-end)
