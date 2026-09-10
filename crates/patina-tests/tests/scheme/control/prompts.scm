;; The delimited-continuation API — `make-continuation-prompt-tag`,
;; `call-with-continuation-prompt`, `abort-current-continuation` — on both
;; backends.
;;
;; This file has no oracle, and is registered `*` on both in `DIVERGENCES.tsv`:
;; measured 2026-09-07, all three procedures are absent from chibi and Gauche,
;; so the top-level tag below kills the file before its first row on either.
;; That is deliberate. `control/cps-features.scm` keeps its prompt rows out for
;; exactly this reason — rows that can never be arbitrated do not belong beside
;; rows that can — and the same applies to the rows here, which came from
;; `crates/patina-tests/tests/backend_divergence.rs` (#193). Where an answer
;; is backed by another implementation the row's comment names it: Guile
;; 3.0.11 and Racket 9.3 have the API, and were consulted by hand.
;;
;; Every row here is a **both-backend** assertion, and most used to be a
;; divergence: the tree-walker had no prompt API until 2026-09-04 (issue #169),
;; and the review of the PR that added it filed four VM defects (#176–#179),
;; all since fixed. The one row still open carries a backend-scoped
;; expectation — see `docs/TEST_ORGANIZATION.md` for the mechanism.
;;
;; The 24-shape transfer matrix behind `docs/VM_RUNTIME.md` §5.6 is
;; `control_flow_matrix.rs`, and the rest of `cps_features.rs`'s prompt half
;; is still Rust; both belong here eventually.
;;
;; Every section makes its own tag. All rows share one interpreter, and the
;; "no live prompt remains" rows below abort to a tag expecting nobody to be
;; listening — a prompt leaked by an earlier row would be found, and the
;; failure would land on the wrong row.

(import (scheme base) (scheme lazy) (srfi 64))

(test-begin "prompts")

;; ── The API, on both backends (issue #169, closed 2026-09-04) ──────────────
;;
;; Until 2026-09-04 only the VM had it, and four quarantines stood in the
;; `.rs` file: two pinning the tree-walker's deliberate not-implemented error
;; (#170), one asserting that error's wording, one that `guard` could catch
;; it. They failed together the moment the API landed, which is what a
;; quarantine is designed to do, and became this.

(define t-api (make-continuation-prompt-tag 'p))

(test-equal "a prompt body's value is the call's value" 42
  (call-with-continuation-prompt (lambda () 42) t-api (lambda (v k) v)))

(test-equal "an abort reaches the prompt's handler" '(handler ab)
  (call-with-continuation-prompt
    (lambda () (abort-current-continuation t-api 'ab))
    t-api (lambda (v k) (list 'handler v))))

;; An abort with no prompt to go to is an ordinary raised error, so a program
;; can catch it — the property the #170 error had and its replacement had to
;; keep. The VM raises `NoMatchingPrompt` through the same route as any
;; runtime error.
(test-equal "an abort with no matching prompt is catchable" '(caught #t)
  (guard (e (#t (list 'caught (error-object? e))))
    (abort-current-continuation t-api 'nowhere)))

;; Two answers the review of #175 found the backends giving differently, both
;; fixed on the side that was wrong. Extra arguments to
;; `call-with-continuation-prompt` go to the body, which is Racket's signature
;; (`proc [prompt-tag handler] arg ...`); the tree-walker rejected them with an
;; arity error and the VM dropped them, so a one-argument body was called with
;; none. And `continuation?` answers #t for a continuation whichever backend
;; captured it: the VM's are `VmContinuationRef`s, which the predicate did not
;; know.
(test-equal "extra arguments go to the body" '(body 1 2)
  (call-with-continuation-prompt (lambda (a b) (list 'body a b)) t-api (lambda (v k) v) 1 2))

(test-equal "the handler's k is a continuation" '(#t #t)
  (call-with-continuation-prompt
    (lambda () (abort-current-continuation t-api 'x))
    t-api (lambda (v k) (list (procedure? k) (continuation? k)))))

(test-equal "so is call/cc's" '(#t #t)
  (call/cc (lambda (c) (list (procedure? c) (continuation? c)))))

;; ── VM defects found by the review of #175 (issues #176–#179) ──────────────
;;
;; Guile 3.0.11 backs the tree-walker's answer wherever it is cited.

;; A prompt does not outlive its body when a continuation captured inside the
;; body is re-entered — issue #176, fixed 2026-09-05.
;;
;; The body's value has been delivered twice, once normally and once through
;; the re-entered continuation, so no prompt is live when the abort runs and
;; `guard` catches it. The tree-walker and Guile 3.0.11 always answered this;
;; the VM died with `expected a procedure, got null`, aborting to a prompt
;; whose body had returned in an earlier top-level form.
;;
;; `(call/cc …)` is in **tail position** of the prompt body, which is what
;; made it reachable: the body's frame is popped before the capture, so the
;; prompt's `stack_depth` already equals `frames.len()` while the body is
;; still running. An abort at that moment must still find the prompt — the
;; tail-position row below, which **Guile 3.0.11 and Racket 9.3** both answer
;; (h x) — and the same depth reading holds once the body is finished. No
;; comparison of depths tells the two apart.
;;
;; So the prompt rides into the continuation's snapshot, and no later sweep
;; reaches it: the frames that would have crossed its depth are gone. The
;; arrival of a full continuation is the one moment the reading *is* exact —
;; the value is being delivered right then, so a prompt with no frame above
;; it has already delivered its own — and that is where the snapshot's
;; resolved prompts are dropped (`restore_continuation`).
;;
;; Both spellings are asserted: the top-level-forms version from the issue,
;; which re-enters a continuation captured in one top-level form from a later
;; one, and the same sequence inside a single `let` body. The first fix tried
;; closed only the first — it truncated the prompt stack when a dispatch loop
;; returned, and inside one loop no loop returns between the re-entry and the
;; abort.

(define t-176 (make-continuation-prompt-tag 'p))
(define saved-176 #f)
(define n-176 0)
(define r-176 (call-with-continuation-prompt
                (lambda () (call/cc (lambda (c) (set! saved-176 c) 'first)))
                t-176 (lambda (v k) (list 'h v))))
(set! n-176 (+ n-176 1))
(if (= n-176 1) (saved-176 'second))

(test-equal "a prompt does not outlive a re-entered body: top-level forms"
  '(r second no-prompt)
  (list 'r r-176 (guard (e (#t 'no-prompt)) (abort-current-continuation t-176 'stale))))

(test-equal "a prompt does not outlive a re-entered body: one let body"
  '(r second no-prompt)
  (let ((saved #f) (n 0))
    (let ((r (call-with-continuation-prompt
               (lambda () (call/cc (lambda (c) (set! saved c) 'first)))
               t-176 (lambda (v k) (list 'h v)))))
      (set! n (+ n 1))
      (if (= n 1) (saved 'second))
      (list 'r r (guard (e (#t 'no-prompt)) (abort-current-continuation t-176 'stale))))))

;; The window the #176 fix must not close: a prompt whose body has
;; **tail-called** away is still the prompt an abort belongs to, even though
;; its `stack_depth` already equals `frames.len()`.
;;
;; Its own row rather than a second assertion in the one above, so that a
;; regression in either direction is reported on its own. **Guile 3.0.11 and
;; Racket 9.3** both answer (h x).
(test-equal "a tail-position prompt is still live" '(h x)
  (call-with-continuation-prompt
    (lambda () (call/cc (lambda (c) (abort-current-continuation t-176 'x))))
    t-176 (lambda (v k) (list 'h v))))

;; An abort out of a Rust primitive's callback reaches its prompt — issue
;; #177, fixed 2026-09-05.
;;
;; `force` runs its thunk through `ApplyContext::apply_proc`, a re-entry
;; boundary with a nested dispatch loop under it. An abort there is not a
;; return: it cuts every stack back to its prompt and pushes one stub frame
;; that has yet to run, so the primitive must be abandoned rather than handed
;; a value.
;;
;; The **tail** spelling is the one that broke. The tail call pops the prompt
;; body's frame before `force` runs, so the abort's landing sits at exactly
;; the depth a callback returning normally would leave — and every check on
;; the way out was a frame-depth comparison. `force` cached the abort's value
;; as its thunk's result and ran on, over the registers the landing was about
;; to use: `expected a procedure, got object`. The non-tail spelling leaves a
;; frame behind and always worked, which is what made the depth reading look
;; sufficient.

(define t-177 (make-continuation-prompt-tag 'p))

(test-equal "an abort out of a primitive's callback reaches its prompt: tail" '(h a1)
  (call-with-continuation-prompt
    (lambda () (force (delay (abort-current-continuation t-177 'a1))))
    t-177 (lambda (v k) (list 'h v))))

(test-equal "an abort out of a primitive's callback reaches its prompt: non-tail" '(h a3)
  (call-with-continuation-prompt
    (lambda () (list 'y (force (delay (abort-current-continuation t-177 'a3)))))
    t-177 (lambda (v k) (list 'h v))))

;; Tree-walker: an abort out of a callback reached through a *nested
;; trampoline* does not find its prompt.
;;
;; The VM answers this since #177. The tree-walker's `apply_from_direct_tagged`
;; — the trampoline a Rust primitive's callback runs on — starts every stack
;; empty, so the abort searches a prompt stack that has none of the caller's
;; prompts and raises "no matching prompt tag", which the wrapper `guard` then
;; catches: (caught #t) where the VM says (h x). `dynamic-wind` records and
;; exception handlers have the same hole on that path and predate prompts
;; entirely; it is the "primitive's callback" entry in
;; `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6, and `cps_eval/prompts.rs` names it
;; as inherited rather than added. When the tree-walker starts answering
;; (h x), delete the expectation line and close that entry.
;;
;; `force` is not the probe here: the tree-walker routes it through the CPS
;; evaluator rather than the trampoline, so it answers (h a1) there. A
;; comparator passed to `assoc` does go through the trampoline.
(cond-expand (patina-tree-walker (test-expect-fail 1)) (else))
(test-equal "an abort out of a nested trampoline callback reaches its prompt" '(h x)
  (guard (e (#t (list 'caught (error-object? e))))
    (call-with-continuation-prompt
      (lambda () (assoc 1 '((1 . a)) (lambda (a b) (abort-current-continuation t-177 'x))))
      t-177 (lambda (v k) (list 'h v)))))

;; A composable continuation captured from inside an exception handler
;; carries the handler with it — issue #178, fixed 2026-09-05.
;;
;; Resuming `k` returns from the handler. R7RS 6.11 then reinstalls the
;; handler after a continuable raise, so the second `raise-continuable` must
;; reach it; and raises a secondary exception after a non-continuable one.
;; The VM did neither: both were Rust after a nested dispatch loop, which the
;; resumed frames returned straight past. Guile 3.0.11 answers as both
;; backends now do.

(define t-178 (make-continuation-prompt-tag 'p))

(test-equal "a composable continuation captured inside a handler carries it: continuable"
  '(h (body resumed (handled-again rc2)))
  (guard (e (#t (list 'caught e)))
    (call-with-continuation-prompt
      (lambda ()
        (with-exception-handler
          (lambda (e) (if (eq? e 'rc)
                          (abort-current-continuation t-178 (list 'aborted e))
                          (list 'handled-again e)))
          (lambda () (list 'body (raise-continuable 'rc) (raise-continuable 'rc2)))))
      t-178 (lambda (v k) (list 'h (k 'resumed))))))

(test-equal "a composable continuation captured inside a handler carries it: non-continuable"
  '(caught #t)
  (guard (e (#t (list 'caught (error-object? e))))
    (call-with-continuation-prompt
      (lambda ()
        (with-exception-handler
          (lambda (e) (abort-current-continuation t-178 (list 'aborted e)))
          (lambda () (list 'body (raise 'nc)))))
      t-178 (lambda (v k) (list 'h (k 'resumed))))))

;; A prompt is closed by *identity*, not by taking the top of the stack, and
;; a dispatch loop closes the prompts it did not open.
;;
;; Both are crashes the review of #179 found, and both need a body that
;; finishes **without a frame** — the case that made
;; `call-with-continuation-prompt` close its own prompt at all.
;;
;; The first: such a body can still re-enter the VM and leave a prompt above
;; this call's. `pop()` then took *that* one, leaving this call's live for the
;; next abort to land on — `expected a procedure, got null`, reading a handler
;; out of a frame that had been cut away. `truncate(prompt_idx)` closes the
;; frame this call pushed and anything the body stranded on top of it.
;;
;; The second is older and independent of the body: a prompt opened inside a
;; nested dispatch loop outlives it, because the sweep at a loop's own exit
;; depth deliberately pops nothing. The handler half of that backstop has
;; always been there; the prompt half was not, and a parameter converter that
;; opens a prompt is enough to reach it with no prompt body in sight.

(define t-179 (make-continuation-prompt-tag 'p))
(define t-179b (make-continuation-prompt-tag 'q))

;; The body is the primitive `assoc`, whose comparator opens a prompt of its
;; own — so when `assoc` returns, the stack top is not this call's.
(test-equal "a no-frame body closes this call's prompt and no other" '((2 b) no-prompt)
  (let ()
    (define (cmp a b)
      (call-with-continuation-prompt (lambda () (equal? a b)) t-179b (lambda (v k) 'h2)))
    (let ((r (call-with-continuation-prompt assoc t-179 (lambda (v k) (list 'H v))
                                            2 '((1 a) (2 b)) cmp)))
      (list r (guard (e (#t 'no-prompt)) (abort-current-continuation t-179 'stale))))))

;; No prompt body at all: a parameter converter opens one inside the nested
;; loop its call runs on, and the loop must close it on the way out.
(test-equal "a prompt opened by a parameter converter is closed by the loop that ran it"
  '(420 no-prompt)
  (let ((q (make-parameter 1
             (lambda (v) (call-with-continuation-prompt (lambda () (* v 10)) t-179b
                                                        (lambda (a k) 'h2))))))
    (q 42)
    (list (q) (guard (e (#t 'no-prompt)) (abort-current-continuation t-179b 'stale)))))

;; The prompt body may be a primitive or a parameter object — issue #179,
;; fixed 2026-09-05.
;;
;; **Racket 9.3 and Guile 3.0.11** both take those, and so did the
;; tree-walker; the VM called the body through `call_closure`, which accepts
;; a compiled closure and nothing else. It goes through `call_any` now, the
;; same dispatcher that already served the prompt's *handler*.
;;
;; What the change costs is the one thing worth remembering here: a primitive
;; or a parameter object finishes without a frame, so no `Return` comes to
;; carry the prompt off by depth, and `call-with-continuation-prompt` closes
;; its own prompt in that case.

(test-equal "the prompt body may be a parameter object" 7
  (call-with-continuation-prompt (make-parameter 7)))

(test-equal "the prompt body may be a primitive, given the arguments after the handler" 6
  (call-with-continuation-prompt + t-179 (lambda (v k) v) 1 2 3))

;; …and the prompt does not outlive such a body: nothing is left for a later
;; abort to find, which is the half a depth sweep cannot do here.
(test-equal "and the prompt does not outlive a frameless body" '(9 no-prompt)
  (list (call-with-continuation-prompt (make-parameter 9) t-179 (lambda (v k) 'h))
        (guard (e (#t 'no-prompt)) (abort-current-continuation t-179 'stale))))

;; A closure body still reaches its handler through an abort, which is the
;; path that does get a frame.
(test-equal "a closure body still reaches its handler through an abort" '(h ab)
  (call-with-continuation-prompt
    (lambda () (abort-current-continuation t-179 'ab)) t-179 (lambda (v k) (list 'h v))))

;; A VM-intercepted **control** primitive as the prompt body — issue #186,
;; closed 2026-09-05.
;;
;;   (call-with-continuation-prompt apply t (lambda (v k) 'h) + '(1 2 3))
;;   tree-walker => 6
;;   VM          => Undefined variable: patina.internal.control/apply
;;
;; Not introduced by #179 and not its to fix: `call_any` probed primitive →
;; parameter → continuation → closure, and a control primitive is claimed by
;; name *before* any of that, so it matched nothing and fell through to a
;; registry lookup that missed. Every caller of that dispatcher had the same
;; hole — `call-with-values`' consumer already did, and #179 made the prompt
;; *body* a second. `call_any` holds no probe of its own now; it is
;; `call_value` plus a frame-depth test, so a callee is callable here exactly
;; when it is callable from a `Call` instruction. The sibling rows are
;; `callability.scm`'s "apply as call-with-values' consumer" rows.

(define t-186 (make-continuation-prompt-tag 'p))

;; `apply`, whose callee needs a frame the prompt must not close early.
(test-equal "a control primitive can be the prompt body: apply" 6
  (call-with-continuation-prompt apply t-186 (lambda (v k) 'h) + '(1 2 3)))

;; `dynamic-wind`, which pushes a stub frame of the VM's own.
(test-equal "a control primitive can be the prompt body: dynamic-wind" 2
  (call-with-continuation-prompt dynamic-wind t-186 (lambda (v k) 'h)
    (lambda () 1) (lambda () 2) (lambda () 3)))

;; `values`, which needs no frame at all — the case that makes
;; `call-with-continuation-prompt` close its own prompt.
(test-equal "a control primitive can be the prompt body: values" 5
  (call-with-continuation-prompt values t-186 (lambda (v k) 'h) 5))

;; …and that prompt is gone: nothing is left for a later abort to land on.
(test-equal "and the prompt a frameless control primitive ran under is gone"
  '(5 no-prompt)
  (list (call-with-continuation-prompt values t-186 (lambda (v k) 'h) 5)
        (guard (e (#t 'no-prompt)) (abort-current-continuation t-186 'stale))))

;; A body that aborts to the prompt this very call pushed.
(test-equal "a control primitive can be the prompt body: abort-current-continuation itself"
  '(h ab)
  (call-with-continuation-prompt abort-current-continuation t-186
    (lambda (v k) (list 'h v)) t-186 'ab))

(test-end)
