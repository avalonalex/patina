;; What happens when a `dynamic-wind` thunk raises during a continuation jump.
;;
;; The rule is R7RS 6.10: "The before and after thunks are called in the same
;; dynamic environment as the call to dynamic-wind" — and 6.11 puts the
;; exception-handler stack in that environment. So a thunk that a `guard`'s
;; escape runs still sees the guard's handler, the handler fires a second time,
;; and its second jump abandons the first: the after-thunk's exception
;; **replaces** the one in flight, unwinding **continues** through the outer
;; winds, and the replacement reaches the nearest handler enclosing the
;; `dynamic-wind`. That is Java's `finally` rule.
;;
;; Migrated whole from `crates/patina-tests/tests/wind_thunk_exceptions.rs`
;; (#193 Phase 1). Nothing had to stay behind: every row was one
;; `assert_program_eval_to`.
;;
;; **Gauche is the oracle for this file.** It implements the rule consistently
;; across every shape below and agrees with all twelve rows.
;;
;; **chibi arbitrates almost none of it**, unlike the sibling
;; `internal-escape-boundaries.scm`, where ordering one row last recovers ten.
;; Measured per row, 2026-09-07: cases 1, 2 and 4 do not terminate at all; case
;; 3 — the normal-exit shape, the one that never needed a fix — completes and
;; agrees. Ordering buys nothing here because the hangs are the rule and not the
;; exception, and a hang is worse than an error: it has to be killed by hand
;; rather than stopping the file. These are chibi's limits on deep
;; `dynamic-wind`/continuation interaction, not disagreements about the rule.
;;
;; **Every row raises a bare symbol, and that is deliberate** — it is what the
;; defect reports and the Gauche comparisons used. The cost is that a regression
;; letting one escape its guard stops the whole file rather than failing one
;; row: SRFI 64's `false-if-error` calls `error-object-message` on whatever was
;; raised, which on a symbol is itself a type error (`scheme_suite.rs` documents
;; this edge). The usual remedy — an enclosing `guard` per row — is **not**
;; available here: it would add a handler to the very stack these rows measure,
;; and case 1 wrapped in a guard is literally case 2, which has a different
;; expected answer. Detection is not lost, only its granularity.
;;
;; **Both backends meet the rule on every shape here**, by the same two
;; mechanisms: a wind record captures the handler stack of its `dynamic-wind`
;; call, and a jump runs each thunk as a step of the machine it was made on — a
;; trampoline step on the tree-walker, a frame under a one-instruction stub
;; (`ResumeWindJump`) on the VM — under that stack. Two further shapes that only
;; the VM ever got wrong — a handler that is *extra* at the jump, and a
;; continuation captured inside a running after-thunk — are in
;; `backend_divergence.rs`.
;;
;; What neither backend covers is a raise from inside a Rust primitive's
;; callback within the thunk — `member`/`assoc` with a predicate,
;; `call-with-port`, `force`, a parameter converter, anything under `eval` —
;; which still runs on a nested trampoline (tree-walker) or a nested dispatch
;; loop (VM) with no handlers of its own (PRD §6, "a primitive's callback runs
;; on a nested trampoline with no handler stack"). No shape below crosses one.
;;
;; Tracked in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6, "An exception raised by a
;; `dynamic-wind` after-thunk does not behave like a `finally`".

(import (scheme base) (srfi 64))

(test-begin "wind-thunk-exceptions")

;; ── The four base cases ─────────────────────────────────────────────────────

;; Case 1 — a `guard` catches, and the after-thunk raises during its escape.
;;
;; Gauche: `(one secondary)`. The clause runs **once** and sees the *secondary*;
;; the primary is discarded, exactly as a Java `finally` discards the exception
;; it replaces. The guard's handler fires twice — once for the primary, again
;; for the secondary raised by its own escape — because the after-thunk runs
;; under the handler stack of the `dynamic-wind` call, where that handler is
;; still installed.
;;
;; The VM used to run the thunk with the handler already popped, so the
;; secondary cascaded uncaught and the program stopped, naming `secondary`. (And
;; before triage families 22 and 28 it answered `(one primary)`: the
;; after-thunk's exception silently discarded, which is the one answer the rule
;; most clearly forbids.)
(test-equal "case 1: after thunk raises during a guard escape" '(one secondary)
  (guard (e (#t (list 'one e)))
    (dynamic-wind (lambda () #f)
                  (lambda () (raise 'primary))
                  (lambda () (raise 'secondary)))))

;; Case 2 — nested guards; the inner one catches the primary.
;;
;; Gauche: `(inner secondary)` — the handler nearest the `dynamic-wind`. The VM
;; used to give the *outer* guard, because the inner one's handler was gone by
;; the time the after-thunk ran.
(test-equal "case 2: nested guards see the after thunk exception" '(inner secondary)
  (guard (o (#t (list 'outer o)))
    (guard (i (#t (list 'inner i)))
      (dynamic-wind (lambda () #f)
                    (lambda () (raise 'primary))
                    (lambda () (raise 'secondary))))))

;; Case 3 — the body exits normally and the after-thunk raises.
;;
;; Both backends always matched Gauche: on a normal exit the live handler stack
;; *is* the `dynamic-wind` call's. Kept so neither fix could regress the one
;; shape that never needed one.
(test-equal "case 3: after thunk raises on a normal exit" '(normal-exit secondary)
  (guard (e (#t (list 'normal-exit e)))
    (dynamic-wind (lambda () #f)
                  (lambda () 'body-ok)
                  (lambda () (raise 'secondary)))))

;; Case 4 — an ordinary `call/cc` escape, with the after-thunk raising.
;;
;; Both match Gauche, and both always did. A plain escape pops no handler, so
;; the VM's live stack happened to equal the call's; the tree-walker used to run
;; the thunk on a nested trampoline with no handlers at all.
(test-equal "case 4: after thunk raises during a call/cc escape" '(escape secondary)
  (guard (e (#t (list 'escape e)))
    (call-with-current-continuation
      (lambda (k)
        (dynamic-wind (lambda () #f)
                      (lambda () (k 'escaped))
                      (lambda () (raise 'secondary)))))))

;; ── Unwinding continues past the thunk that raised ──────────────────────────

;; Nested winds, where the *inner* after-thunk raises: unwinding continues
;; outward past it.
;;
;; Gauche: `((caught sec2) (in1 in2 out2 out1))`. The guard's second jump starts
;; from the wind stack the first jump had got to — the inner record already
;; popped — so it runs `out1` and then delivers `sec2`.
(define nested-log '())
(test-equal "nested winds finish unwinding before delivering the replacement"
  '((caught sec2) (in1 in2 out2 out1))
  (let* ((caught
          (guard (e (#t (list 'caught e)))
            (dynamic-wind (lambda () (set! nested-log (cons 'in1 nested-log)))
              (lambda ()
                (dynamic-wind (lambda () (set! nested-log (cons 'in2 nested-log)))
                  (lambda () (raise 'primary))
                  (lambda () (set! nested-log (cons 'out2 nested-log)) (raise 'sec2))))
              (lambda () (set! nested-log (cons 'out1 nested-log))))))
         (entered (reverse nested-log)))
    (list caught entered)))

;; The same program wrapped in an outer guard, which is what separated the two
;; halves of the old VM answer: the VM always finished the unwind when something
;; outside caught (`(in1 in2 out2 out1)` either way), but delivered to the outer
;; guard, one handler too far out — case 2's gap. With a single guard it stopped
;; at `sec2` and never ran `out1`, because nothing caught and an unhandled
;; exception stops the program where it is raised. Both forms are kept: the pair
;; is what says the unwind was never the defect, only who catches.
(define outer-log '())
(test-equal "…and an enclosing guard does not change where it is delivered"
  '((caught sec2) (in1 in2 out2 out1))
  (let* ((caught
          (guard (o (#t (list 'outer o)))
            (guard (e (#t (list 'caught e)))
              (dynamic-wind (lambda () (set! outer-log (cons 'in1 outer-log)))
                (lambda ()
                  (dynamic-wind (lambda () (set! outer-log (cons 'in2 outer-log)))
                    (lambda () (raise 'primary))
                    (lambda () (set! outer-log (cons 'out2 outer-log)) (raise 'sec2))))
                (lambda () (set! outer-log (cons 'out1 outer-log)))))))
         (entered (reverse outer-log)))
    (list caught entered)))

;; ── Handlers around and inside the body ─────────────────────────────────────

;; A handler *inside* the body re-raises, then the after-thunk raises.
;;
;; Gauche: `(outer secondary)`. Two handlers fire for the primary (the inner one
;; re-raises to the guard); the after-thunk still runs under the stack the
;; `dynamic-wind` call had, which holds the guard. On the VM both handlers had
;; been popped by then, and the secondary cascaded uncaught.
(test-equal "inner handler reraises, then the after thunk raises" '(outer secondary)
  (guard (e (#t (list 'outer e)))
    (dynamic-wind (lambda () #f)
      (lambda ()
        (with-exception-handler
          (lambda (c) (raise (list 'from-inner c)))
          (lambda () (raise 'primary))))
      (lambda () (raise 'secondary)))))

;; `raise-continuable` from an after-thunk, with a handler that returns: the
;; thunk resumes, completes, and the original escape lands.
;;
;; Gauche: `(x (sec))`. Both match, and both did before: a plain escape pops
;; nothing, so even the old "handlers from the machine" rule found the right
;; stack. Both now run the thunk as a step with the call's handlers, so the
;; handler's return goes back into the thunk.
(define continuable-log '())
(test-equal "raise-continuable in an after thunk resumes the thunk" '(x (sec))
  (let* ((r (with-exception-handler
              (lambda (c) (set! continuable-log (cons c continuable-log)) 'ignored)
              (lambda ()
                (call/cc (lambda (k)
                  (dynamic-wind (lambda () #f)
                                (lambda () (k 'x))
                                (lambda () (raise-continuable 'sec))))))))
         (seen (reverse continuable-log)))
    (list r seen)))

;; A `guard` declines, its `handler-k` re-enters the wind, and the *before*-thunk
;; raises on that second entry.
;;
;; Gauche: `(outer before-raised ((before 1) after (before 2)))`. The before
;; thunk runs under the `dynamic-wind` call's stack — both guards — so the inner
;; guard gets a second look, declines again, and the outer one catches. Its
;; record is not on the wind stack while it runs, so the escape does not run the
;; after-thunk a second time: `after` appears once. The VM gives the same answer,
;; and used to reach it by a shorter route: the inner handler was already gone,
;; so the outer one fired directly.
(define reentry-n 0)
(define reentry-log '())
(test-equal "before thunk raises on reentry after a guard declines"
  '(outer before-raised ((before 1) after (before 2)))
  (guard (o (#t (list 'outer o (reverse reentry-log))))
    (guard (e ((eq? e 'never) 'never))
      (dynamic-wind
        (lambda ()
          (set! reentry-n (+ reentry-n 1))
          (set! reentry-log (cons (list 'before reentry-n) reentry-log))
          (if (= reentry-n 2) (raise 'before-raised)))
        (lambda () (raise 'primary))
        (lambda () (set! reentry-log (cons 'after reentry-log)))))))

;; The after-thunk raises and the guard's clause *declines* the secondary: the
;; re-raise reaches the outer guard.
;;
;; Gauche: `(outer secondary)`. Both match — and the VM used to match here by
;; accident, case 2's gap dropping the very handler this row's clause declines
;; to.
(test-equal "a guard clause declining the secondary reaches the outer guard"
  '(outer secondary)
  (guard (o (#t (list 'outer o)))
    (guard (e ((eq? e 'primary) (list 'inner e)))
      (dynamic-wind (lambda () #f)
                    (lambda () (raise 'primary))
                    (lambda () (raise 'secondary))))))

;; No `guard` at all: a `with-exception-handler` handler escapes through a
;; continuation, and the after-thunk raises on the way out. The handler runs
;; again, for the secondary, and its second escape replaces the first.
;;
;; Gauche: `(escaped secondary (primary secondary))`. On the VM the handler had
;; been popped by the first raise and the secondary cascaded uncaught.
(define handler-log '())
(test-equal "a handler escape runs the handler again for the secondary"
  '(escaped secondary (primary secondary))
  (call/cc (lambda (k)
    (with-exception-handler
      (lambda (c)
        (set! handler-log (cons c handler-log))
        (k (list 'escaped c (reverse handler-log))))
      (lambda ()
        (dynamic-wind (lambda () #f)
                      (lambda () (raise 'primary))
                      (lambda () (raise 'secondary))))))))

;; The shape that decided both designs. The inner guard declines the secondary;
;; the outer handler *returns* from the continuable re-raise; so the after-thunk
;; must resume where `handler-k` re-entered it, complete, and let the original
;; escape land.
;;
;; Gauche: `(escaped (secondary))`. Running the thunk as a step of the machine
;; the jump was made on makes that re-entry ordinary, because the thing that owns
;; "the rest of the jump" is a resumable frame — a `Jump` continuation on the
;; tree-walker, a `ResumeWindJump` stub frame on the VM. Running it on a nested
;; Rust call cannot: the nested frame is gone by the time `handler-k` re-enters.
;; The tree-walker's nested trampoline ended the program with `'ignored` at the
;; thunk's `Halt`; the VM re-entered the enclosing frame *inside* the inlined
;; `dynamic-wind` sequence, which ran the after-thunk a second time and lost the
;; escape's value — `(() (secondary secondary))`.
(define deciding-log '())
(test-equal "the after thunk resumes after a declining guard and a returning handler"
  '(escaped (secondary))
  (let* ((r (with-exception-handler
              (lambda (c) (set! deciding-log (cons c deciding-log)) 'ignored)
              (lambda ()
                (guard (e ((eq? e 'never) 'never))
                  (call/cc (lambda (k)
                    (dynamic-wind (lambda () #f)
                                  (lambda () (k 'escaped))
                                  (lambda () (raise-continuable 'secondary)))))))))
         (seen (reverse deciding-log)))
    (list r seen)))

;; **Larceny family 28** (`scheme_tests/reports/larceny_triage.md`), moved here
;; from `larceny_families.rs` because this file is about wind thunks and
;; exceptions meeting.
;;
;; R7RS §6.11: a handler is called "in the dynamic environment of the call to
;; `raise`, except that the current exception handler is the outer one". Both
;; backends unwound to the handler's own wind depth *first*, so the handler ran
;; outside the extent and the after-thunk ran twice. Fixed 2026-09-01.
;;
;; The fix removes work rather than adding it: a `raise` crosses no dynamic
;; extent, so no raise path unwinds any more. Popping the handler stack is the
;; only thing R7RS asks a raise to change; `guard`'s `guard-k` does the
;; crossing, through the same wind machinery as every other control transfer.
;;
;; Found 2026-08-25 while attempting family 22, whose visible symptom this
;; produces on the non-continuable path. Neither half fixed it alone, which was
;; measured rather than deduced: R7RS §7.3's reference `guard` jumps a
;; continuation back to the raise point and still gave family 22's wrong answer
;; under the old raise path, because that continuation was captured after the
;; unwind.
;;
;; Two neighbours deliberately not re-pinned here, both in
;; `backend_divergence.rs`: the tree-walker's `error` path, whose opposite
;; ordering converged with this (`a_guard_clause_runs_after_the_unwind`), and
;; the VM wind machinery the fix leans on, which #149 converged
;; (`a_continuation_within_its_own_wind_runs_the_thunks_once`). Plain re-entry
;; is `compliance/control.rs`'s `test_dynamic_wind_with_callcc_reentry` (R7RS
;; §6.10's own example) rather than re-derived here.
;;
;; All three implementations agree on this one.
;; Top level, like this file's other seven logs: `docs/TEST_ORGANIZATION.md`
;; asks migrations to keep `define`s where they were, because an internal one
;; is `letrec*` on a local slot and a row can be moved off the path it was
;; written for without anything saying so.
(define extent-log '())
(define (extent-note x) (set! extent-log (cons x extent-log)))
(define extent-answer
  (with-exception-handler
    (lambda (e) (extent-note 'handler) 'handled)
    (lambda ()
      (dynamic-wind (lambda () (extent-note 'in))
                    (lambda () (raise-continuable 'x))
                    (lambda () (extent-note 'out))))))
(test-equal "a handler runs inside the raise's dynamic extent"
  '(handled (in handler out))
  (list extent-answer (reverse extent-log)))

(test-end)
