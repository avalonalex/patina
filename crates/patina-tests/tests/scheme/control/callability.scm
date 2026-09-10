;; Which sites decide "is this callable", and what they decide.
;;
;; Migrated from `crates/patina-tests/tests/callability.rs` (#193 Phase 0). The
;; rows that name a single backend stayed there; everything here holds on both,
;; and on chibi and Gauche, which is why it can be a plain SRFI 64 program.
;;
;; This file exists because prose about those sites kept being wrong. Reviewing
;; the `procedure?`-on-parameters fix turned up three claims in its own commit
;; message that no test could have contradicted: that `dynamic-wind` validates
;; its arguments (it does not), that a certain count of call sites "became
;; correct together" (it was counting grep hits, not decisions), and that
;; `Heap::is_procedure` had become the single source of truth for callability
;; (it has not). Each was a statement about observable behaviour, so each is
;; pinned below.
;;
;; The rule these encode: a claim about *which* check runs is testable by
;; ordering or by what is accepted, without depending on error text — error
;; messages are not a stable interface, and two sites here share one message
;; verbatim.
;;
;; A second rule, learned the same way: "the other backend does X" is not a
;; reason to believe X — the convergence rows below were checked against chibi,
;; Gauche and Chez before the tree-walker was changed to agree with the VM.
;;
;; A third: a defect class cannot be enumerated by grep. The first sweep of
;; `cps_eval/application.rs` matched `return Err(…)` and missed three catchable
;; errors that reach Rust through `?`.
;;
;; `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 carries the history.

(import (scheme base) (srfi 64))

(test-begin "callability")

;; ── What validates its arguments, and what merely calls them ────────────────

;; `dynamic-wind` performs no up-front validation on either backend — neither
;; `apply_dynamic_wind` nor `VmControlPrimitive::DynamicWind` inspects its
;; arguments, they just call the thunks.
;;
;; Proven by ordering rather than by an error message: if the arguments were
;; checked first, nothing would have run. Because `before` and `body` both run
;; before the bad after-thunk is reached, the failure is a call, not a check.
;;
;; Why it is worth a test: a draft of the parameter fix cited `dynamic-wind` as
;; one of the sites that "became correct" when the callability predicate
;; widened, and wrote a test to prove it. That test passed against unfixed
;; code, because `dynamic-wind` accepts anything until it tries to call it.
(test-equal "dynamic-wind does not validate its arguments"
  '(before body)
  (let ((log '()))
    (guard (e (#t (reverse log)))
      (dynamic-wind (lambda () (set! log (cons 'before log)))
                    (lambda () (set! log (cons 'body log)) 'ok)
                    5))))

;; `with-exception-handler`, by contrast, *is* a real decision point: it
;; rejects a non-procedure instead of discovering the problem when it calls.
;; This is the check that a parameter object failed before `procedure?` was
;; fixed, on both backends.
;;
;; **This is stricter than chibi and Gauche**, deliberately. Both accept a
;; non-procedure handler and just run the thunk, returning `ok` — they only
;; care when an exception is actually raised and the handler is called. Chez
;; rejects it as Patina does. R7RS leaves the case unspecified, so this is a
;; choice, not conformance; it is recorded here so nobody "fixes" Patina to
;; match chibi without knowing Chez sits on the other side.
(test-error "with-exception-handler rejects a non-procedure handler" #t
  (with-exception-handler 5 (lambda () 'ok)))
(test-error "with-exception-handler rejects a non-procedure thunk" #t
  (with-exception-handler (lambda (e) e) 5))
;; And it accepts what `procedure?` accepts — the property the parameter fix
;; restored. `tests/parameters.rs` covers the parameter case.
(test-equal "with-exception-handler accepts procedures"
  'ok
  (with-exception-handler (lambda (e) e) (lambda () 'ok)))

;; ── Errors raised by control primitives are catchable, in every position ────

;; Converged 2026-08-15: an error raised *by a control primitive itself* is a
;; catchable condition, in every position, on both backends.
;;
;; Same class as #71 — a catchable error returned as a Rust `Err` instead of
;; routed through the Scheme handlers — in the file that fix did not reach.
;;
;; Checked against chibi, Gauche and Chez first. They differ on whether some of
;; these should raise *at all*, never on catchability once something is raised
;; — so a future change here is about the former, not the latter. The PRD
;; carries the measured table.
;;
;; Each body is asserted twice: caught when guarded, and *still an error* when
;; not. Routing changes where a catchable error is delivered, never whether it
;; is raised, and asserting only the first half would not notice a fix that
;; swallowed errors instead of routing them.
;;
;; Two of these rows exist because the first sweep missed them. It was defined
;; syntactically — "every bare `return Err(…)`" — while three catchable errors
;; in the same file reach Rust through `?` instead. `(error 5)` is one: fifteen
;; lines below an arity check the sweep did route, in the same function. Grep
;; patterns are not a way to enumerate a defect class.
;;
;; Only the *guarded* halves are here. Their unguarded partners — the same
;; bodies run as bare top-level programs, required to fail — stayed in
;; `callability.rs`, because `test-error` runs its body inside `call/cc` and
;; `with-exception-handler` (SRFI 64's `%test-error`), which is the very
;; routing the guarded row already checks. Asserting both here would make each
;; pair a near-duplicate, and a regression that routed these errors to handlers
;; while breaking the unguarded top-level path — the exact shape of defect #71
;; this section cites — would pass both halves. Whether an error escapes an
;; unguarded program is observable only from outside it, which is what makes it
;; the Rust half's job.

(test-equal "catchable: handler is not a procedure"
  'caught (guard (e (#t 'caught)) (with-exception-handler 5 (lambda () 'ok))))
(test-equal "catchable: dynamic-wind arity"
  'caught (guard (e (#t 'caught)) (dynamic-wind (lambda () 1))))
(test-equal "catchable: call-with-values arity"
  'caught (guard (e (#t 'caught)) (call-with-values (lambda () 1))))
(test-equal "catchable: raise arity"
  'caught (guard (e (#t 'caught)) (raise)))
(test-equal "catchable: error arity"
  'caught (guard (e (#t 'caught)) (error)))
(test-equal "catchable: a parameter's own arity"
  'caught (guard (e (#t 'caught)) ((make-parameter 1) 1 2 3)))
(test-equal "catchable: error message is not a string"
  'caught (guard (e (#t 'caught)) (error 5)))
(test-equal "catchable: error message is a symbol, the chibi-lenient shape"
  'caught (guard (e (#t 'caught)) (error 'sym)))

;; An error raised by user code inside a `dynamic-wind` after thunk reaches the
;; enclosing `guard` — converged 2026-09-01.
;;
;; It used to escape on the tree-walker: `run_wind_handlers(…)?` ran the thunk
;; on a nested trampoline with an empty handler stack, so the error had to come
;; back through Rust and nothing routed it to the handlers installed outside.
;; Wind thunks now run as steps of the trampoline the jump was made on, each
;; under the handler stack its `dynamic-wind` call was made in (R7RS 6.10;
;; `cps_eval/wind.rs`), so the `(car 7)` error finds the `guard` like any other
;; raise would. Primitive callbacks still run on the nested trampoline — that
;; boundary stays open in `cps-features.scm`, `escape_from_primitive.rs` and §6.
;;
;; `caught` is arbitrated by Gauche (chibi loops forever on this program): the
;; after thunk runs in the environment of the `dynamic-wind` call, which is
;; inside the `with-exception-handler`, but that handler's `'handled` is then a
;; return from a non-continuable `raise`, which R7RS 6.11 makes a secondary
;; exception raised in the same environment — so the `guard` catches either
;; way. The VM's half moved from `handled` to `caught` with the audit's A3 fix
;; (wind records are popped before their after-thunk runs); `handled` came from
;; the old ordering swallowing the thunk's error.
;; **Scoped away from chibi, which loops forever on this program.** Not a
;; disagreement about the answer — chibi never produces one, and without the
;; skip the file times out and chibi arbitrates none of the other 25 rows.
;; `test-skip` prevents *evaluation*, so the program never runs there.
;; Measured 2026-09-09: with this one row skipped chibi completes the file.
(cond-expand (chibi (test-skip 1)) (else))
(test-equal "an error in a wind thunk reaches the enclosing guard"
  'caught
  (guard (e (#t 'caught))
    (with-exception-handler (lambda (c) 'handled)
      (lambda ()
        (dynamic-wind (lambda () 1)
                      (lambda () (raise 'x))
                      (lambda () (car 7)))))))

;; ── `apply`'s callee set is `Call`'s callee set ─────────────────────────────

;; `apply` used as a value, which is what started this section.
;;
;; The desugarer intercepts `apply` in head position and lowers it to a
;; dedicated instruction, so `(apply f xs)` never consults the binding. Reached
;; any other way — through a variable, an argument, a higher-order procedure —
;; it resolved to the `apply` that `(patina internal control)` exports, and the
;; VM then dispatched it through the primitive registry, where nothing
;; implements it: spreading a list into a real call is work only the VM can do.
;; So the VM reported `Undefined variable: patina.internal.control/apply` while
;; the tree-walker, which intercepts `apply` by name at call time, answered 6.
(test-equal "apply as a value" 6 (let ((f apply)) (f + '(1 2 3))))
;; Through a higher-order procedure, the shape real code hits.
(test-equal "apply through a higher-order procedure" '(3)
  (map (lambda (f) (f + '(1 2))) (list apply)))
;; In tail position, which takes a different dispatcher.
(test-equal "apply in tail position" 15
  ;; `define` deliberately, not `let`: the row's point is the dispatcher a
  ;; *global* callee takes, and the VM compiles a global reference differently
  ;; from a local slot.
  (let () (define (call-it g) (g + '(7 8))) (call-it apply)))
;; Fixed arguments before the spread list.
(test-equal "apply with fixed arguments" 10 (let ((f apply)) (f + 1 2 '(3 4))))

;; The deeper half of the same fix, and the reason it is here rather than in a
;; test named after `apply`: both apply instructions probed only
;; primitive → parameter → closure, so *`apply`'s* idea of what is callable was
;; narrower than `Call`'s. Every callee below is accepted by a direct call and
;; was rejected through `apply`.
;;
;; Scoped to the two apply *instructions* on purpose — the `call_any`
;; dispatcher is the section below.
;;
;; Verified against chibi and Gauche, which accept all of them.

;; A VM-intercepted control primitive.
(test-equal "apply of with-exception-handler" 43
  (apply with-exception-handler
         (list (lambda (e) 43) (lambda () (raise-continuable 'x)))))
(test-equal "apply of dynamic-wind" 2
  (let () (define r '())
    (apply dynamic-wind
           (list (lambda () (set! r 1)) (lambda () 2) (lambda () (set! r 3))))))
;; `apply` itself is one of them, so this is also the self-application case.
(test-equal "apply of apply" 3 (apply apply (list + '(1 2))))
;; A parameter object — the one callee kind the old code already handled, and
;; covered on its own in `parameters.rs`. Kept to complete the set.
(test-equal "apply of a parameter" 5
  (let () (define p (make-parameter 5)) (apply p '())))

;; A continuation reached through `apply`, on both backends.
;;
;; Kept separate because it is easy to conflate with the one case still failing
;; on the tree-walker, and it was conflated once: this was first written as a
;; pinned divergence, and `assert_divergence` rejected it. What the tree-walker
;; still fails is `(apply call/cc …)` — `call/cc` *as apply's callee*, resolved
;; by name in value position — not a continuation object, which it invokes here
;; fine. That one is pinned as the "apply on call/cc" row at the end of this file.
(test-equal "apply invokes a continuation" 42
  (call/cc (lambda (k) (let ((f apply)) (f k '(42))))))

;; The hole the fix above did *not* close, closed 2026-09-05 by issue #186.
;;
;; `apply` reached through `call_any` — the VM's third and narrowest dispatcher
;; — used to fail here. `call_any` had kept the exact
;; primitive → parameter → closure probe that the apply instructions shed, and
;; it is what runs `call-with-values`' consumer, `call/cc`'s procedure, a wind
;; thunk and — since issue #179 made it a caller — a prompt **body**. So the
;; callee set was uniform across the two apply *instructions* and not across
;; the VM.
;;
;; It holds no probe of its own now: it calls `call_value` and reads the frame
;; depth to learn whether the callee finished. The `exit_depth` that an earlier
;; comment named as the obstacle was a parameter nothing read.
;;
;; Found by review, not by the tests: the first version of *that* work claimed
;; "`apply` accepts every callee a direct call accepts", and a five-token
;; program falsified it — with the same error string the change had just
;; declared fixed, one dispatcher over. Which is why the row is here as a
;; program rather than as a sentence.

;; The consumer.
(test-equal "apply as call-with-values' consumer" 3
  (call-with-values (lambda () (values + '(1 2))) apply))
;; …in tail position, which pops the frame before dispatching.
(test-equal "…in tail position" 3
  ((lambda () (call-with-values (lambda () (values + '(1 2))) apply))))
;; …and `call-with-values` itself reached as a value, so the consumer is
;; dispatched from `handle_control_primitive` rather than an instruction.
(test-equal "…with call-with-values itself as a value" 3
  (let ((f call-with-values)) (f (lambda () (values + '(1 2))) apply)))
;; The producer is the same dispatcher: `values` with no arguments.
(test-equal "the producer is the same dispatcher" '()
  (call-with-values values list))

;; ── `call/cc` in value position (Track Q §1.2) ───────────────────────────────
;;
;; Migrated from `crates/patina-tests/tests/backend_divergence.rs` (#193),
;; where they were `assert_divergence` quarantines. Each row asserts the
;; answer R7RS requires; the line above it names the backend known to get it
;; wrong, using the feature identifier that backend advertises (see
;; `docs/TEST_ORGANIZATION.md`). The driver fails the run the day that
;; backend starts passing, and the fix is to delete the line.
;;
;; Shared root cause: R7RS §6.10 makes `call/cc` an ordinary procedure, but
;; the tree-walker claims it *syntactically* (`cps_transform.rs`'s
;; `is_callcc_reference`), so a reference in value position falls through to a
;; registry binding that is not there — `Undefined variable:
;; patina.internal.control/call/cc`. It works when called directly, which is
;; why the 1226/1226 chibi suite never catches it: that suite never takes
;; `call/cc` as a value. Q2 part 1 is the fix — a real binding behind the name.
;; chibi and Gauche answer every row below as the VM does.
;;
;; `let`-bound rather than `define`-bound as the `.rs` file had it: a top-level
;; `(define f call/cc)` raises outside any row on the tree-walker and would
;; take the whole file down instead of costing one expected failure.

(cond-expand (patina-tree-walker (test-expect-fail 1)) (else))
(test-equal "call/cc bound to a variable" 1
  (let ((f call/cc)) (f (lambda (k) 1))))

;; Same root cause, kept separate because passing a control operator *through
;; a higher-order procedure* is the shape real code hits (SRFI 1).
(cond-expand (patina-tree-walker (test-expect-fail 1)) (else))
(test-equal "call/cc passed to a higher-order procedure" '(6)
  (map (lambda (f) (f (lambda (k) 6))) (list call/cc)))

;; Was "fails on both", recorded so Q2 would not mistake backend *agreement*
;; for correctness. Half of it is fixed: the VM evaluates it to 1, as R7RS
;; requires and as chibi does, so what was a shared gap is an ordinary
;; divergence with the tree-walker on the wrong side — the same registry hole
;; as the two rows above, and `apply` is simply a third way to reach it.
(cond-expand (patina-tree-walker (test-expect-fail 1)) (else))
(test-equal "apply on call/cc" 1
  (apply call/cc (list (lambda (k) 1))))

;; Track Q §1.2 recorded this as a VM failure (`Wrong number of arguments:
;; expected 1, got 2`) at `7a6a797`; both backends return 7 as of `2d4ce29`.
;; Kept as the regression guard for a row that was fixed without anyone
;; noticing.
(test-equal "apply on values" 7
  (apply values (list 7)))

(test-end)
