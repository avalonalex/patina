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
;; all since fixed; the tree-walker's last one, an abort out of a primitive's
;; callback, closed 2026-09-10.
;;
;; The 24-shape transfer matrix behind `docs/VM_RUNTIME.md` §5.6 is
;; `control_flow_matrix.rs`, which stays Rust: it is a scoreboard, read as a
;; table. The rest of `cps_features.rs`'s prompt half moved here in #193
;; Phase 2 — the section on aborts, composable continuations and the dynamic
;; state they carry.
;;
;; Every section makes its own tag. All rows share one interpreter, and the
;; "no live prompt remains" rows below abort to a tag expecting nobody to be
;; listening — a prompt leaked by an earlier row would be found, and the
;; failure would land on the wrong row. Those rows sequence the prompt call
;; and the probe with `let*`, never as two arguments of `list`: R7RS leaves
;; argument order unspecified, and a right-to-left evaluator would run the
;; probe first and prove nothing.

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

;; ── Aborts and composable continuations, and the dynamic state they carry ──
;;
;; Moved from `crates/patina-tests/tests/cps_features.rs` (#193 Phase 2), the
;; last of its prompt half. These are cells of the dynamic-state matrix in
;; `docs/VM_RUNTIME.md` §5.6 — of the five components of `VmState` that belong
;; to a dynamic extent rather than to the machine, which each transfer saves,
;; restores or truncates — and each row names the external implementation its
;; answer was measured against: Racket 9.3, Guile 3.0.11, or Gauche 0.9.15's
;; `gauche.partcont`.
;;
;; The Rust tests spelled most of these as several top-level forms, some of
;; them re-entering a continuation captured by an earlier form. Here each
;; scenario is one expression and re-enters at most once, so a wrong jump
;; re-runs the row's own `let` and delivers a wrong value to the row instead
;; of re-running some earlier form and taking the rest of the file with it.
;; Where a row's *depths* matter, capture and resume still happen at depths
;; that differ, which is the property the relocation rows exist for.

;; ─ An abort out of each of the value form's thunks (measured against Racket)
;;
;; Nothing covered the *value* form of `dynamic-wind` under a prompt before
;; 2026-09-02, which is how three defects survived on the VM:
;;
;;                          fixed                        main (7e696892)
;;   body, tail position    ((handler ab) (in b1 out))   same
;;   body, non-tail         ((handler ab) (in b1 out))   panic: "no active frame"
;;   before thunk           ((handler ab) (in))          (#<unspecified> (in body out))
;;   after thunk            ((handler ab) (in body out)) (#<unspecified> (in body out))
;;
;; The body case failed only when the `dynamic-wind` was *not* in tail
;; position of the prompt body — the tail shape tail-pops the frame first and
;; happened to survive — so the non-tail spelling is the one with the signal.
;; The other two `main` rows skipped the handler *and* ran thunks the abort
;; should have prevented. All three follow from the same change as #157/#159:
;; the abort truncates to the prompt's frame depth, and the value form's
;; bookkeeping is frames rather than a Rust call the truncation walked out
;; from under. On both backends since the tree-walker's prompt API (#169).

(define dw-vf dynamic-wind)
(define t-vf (make-continuation-prompt-tag 'value-form))
(define (vf-probe body)
  (let ((log '()))
    ;; `let*`, not `(list (run …) (reverse log))`: argument order is
    ;; unspecified, and reading the log first is a different program.
    (let* ((r (call-with-continuation-prompt
                (lambda () (body (lambda (x) (set! log (cons x log)))))
                t-vf
                (lambda (v k) (list 'handler v))))
           (l (reverse log)))
      (list r l))))

;; From the body: the extent was entered, so `out` still runs.
(test-equal "an abort from the value form's body runs its after thunk: tail"
  '((handler ab) (in b1 out))
  (vf-probe (lambda (note)
              (dw-vf (lambda () (note 'in))
                     (lambda () (note 'b1) (abort-current-continuation t-vf 'ab) (note 'b2))
                     (lambda () (note 'out))))))

(test-equal "an abort from the value form's body runs its after thunk: non-tail"
  '((handler ab) (in b1 out))
  (vf-probe (lambda (note)
              (list 'body-result
                    (dw-vf (lambda () (note 'in))
                           (lambda () (note 'b1) (abort-current-continuation t-vf 'ab) (note 'b2))
                           (lambda () (note 'out)))))))

;; From the before thunk: the extent is not entered until it returns, so
;; neither the body nor the after thunk runs.
(test-equal "an abort from the value form's before thunk enters nothing"
  '((handler ab) (in))
  (vf-probe (lambda (note)
              (dw-vf (lambda () (note 'in) (abort-current-continuation t-vf 'ab))
                     (lambda () (note 'body))
                     (lambda () (note 'out))))))

;; From the after thunk: everything has already run.
(test-equal "an abort from the value form's after thunk"
  '((handler ab) (in body out))
  (vf-probe (lambda (note)
              (dw-vf (lambda () (note 'in))
                     (lambda () (note 'body))
                     (lambda () (note 'out) (abort-current-continuation t-vf 'ab))))))

;; ─ The handler's composable continuation resumes its computation (Racket)
;;
;; The abort rows above deliberately never resume `k`, which is how issue
;; #160 survived. Appending the captured frames was the whole of the invoke:
;; the value went into a register named by one frame's numbering and indexed
;; against another's base, and what the resumed computation returned went to
;; the register the prompt had been going to write — in a frame the abort had
;; already unwound. Neither delivery reached anybody:
;;
;;                                  fixed               main (4624b0e8)
;;   (list 'got ␣) resumed with 10  (got 10)            ()
;;   (+ 1 ␣) resumed with 10        11                  Type error: +
;;   k invoked twice                ((got 1) (got 2))   (() ())
;;
;; Racket gives the fixed column for every row here. Its spelling differs —
;; an abort there passes only values, so the continuation is captured with
;; `call-with-composable-continuation` and carried through the abort — but
;; the programs are otherwise these. The two halves of the fix are each
;; invisible without the other, and the first row separates them: delivering
;; into the hole while leaving the captured return where it was gives
;; `main`'s answer exactly; re-pointing the return without delivering gives
;; `(handler ab resumed-to (got ()))`.

(define t-ck (make-continuation-prompt-tag 'composable))
(define (ck-probe body handler)
  (call-with-continuation-prompt body t-ck handler))

(test-equal "a handler's k resumes the computation with the value in the hole"
  '(handler ab resumed-to (got 10))
  (ck-probe (lambda () (list 'got (abort-current-continuation t-ck 'ab)))
            (lambda (v k) (list 'handler v 'resumed-to (k 10)))))

;; The delivered value is read, not merely dropped: this used to reach `+`
;; with the NULL an untouched register holds.
(test-equal "the value delivered to the hole is the value the computation reads"
  '(ab 11)
  (ck-probe (lambda () (+ 1 (abort-current-continuation t-ck 'ab)))
            (lambda (v k) (list v (k 10)))))

(test-equal "a composable continuation invoked twice runs twice" '((got 1) (got 2))
  (ck-probe (lambda () (list 'got (abort-current-continuation t-ck 'ab)))
            (lambda (v k) (let* ((a (k 1)) (b (k 2))) (list a b)))))

;; More than one frame between the prompt and the abort: the relocation has
;; to keep the appended frames' return chain pointing at each other, and only
;; its outermost frame at the invoke.
(test-equal "a capture of several frames resumes all of them"
  '(handler ab (a (b zz 10)))
  (ck-probe (lambda ()
              (list 'a ((lambda (z) (list 'b z (abort-current-continuation t-ck 'ab))) 'zz)))
            (lambda (v k) (list 'handler v (k 10)))))

;; `k` in tail position of the handler: the invoking frame is popped before
;; the append, so the resumed computation returns past it.
(test-equal "k in tail position of the handler" '(got 10)
  (ck-probe (lambda () (list 'got (abort-current-continuation t-ck 'ab)))
            (lambda (v k) (k 10))))

;; Nothing between the abort and its prompt — the abort is in tail position
;; of the body, so that frame is already popped when the capture happens —
;; makes `k` the identity. Both positions, because the tail one returns
;; through the handler's own frame.
(test-equal "an empty capture is the identity" '((ab 10) 10)
  (list (ck-probe (lambda () (abort-current-continuation t-ck 'ab))
                  (lambda (v k) (list v (k 10))))
        (ck-probe (lambda () (abort-current-continuation t-ck 'ab))
                  (lambda (v k) (k 10)))))

;; Reached through `call_any` rather than an instruction. The value form of
;; `call-with-values` runs its producer across a nested Rust boundary that
;; still owes the consumer a call, and reporting the resumed frames as a
;; continuation *escape* told it to abandon that: the two spellings of one
;; program disagreed, `(handler ())` against the head form's answer. A
;; composable continuation returns, so its invoke reports a pushed frame.
;; `(k)` with no arguments delivers a `#<values>` object exactly as
;; `(values)` would — Patina's printed spelling, compared as written.
(test-equal "call-with-values takes k as its producer in head and value form alike"
  '("(handler ((got #<values>)))" "(handler ((got #<values>)))")
  (let ((written (lambda (x)
                   (let ((port (open-output-string)))
                     (write x port)
                     (get-output-string port))))
        (cwv call-with-values))
    (list (written (ck-probe (lambda () (list 'got (abort-current-continuation t-ck 'ab)))
                             (lambda (v k) (list 'handler (call-with-values k list)))))
          (written (ck-probe (lambda () (list 'got (abort-current-continuation t-ck 'ab)))
                             (lambda (v k) (list 'handler (cwv k list))))))))

;; `k` as the exception handler itself, with *two* frames captured: the raise
;; runs the handler down to the depth it started at, which stops being
;; `frames.len() - 1` as soon as an invoke can push more than one frame.
;; Reading it after the call gave `(handler (a ()))` — the resumed
;; computation cut in half.
(test-equal "k can be the exception handler, over a capture of two frames"
  '(handler (a (b zz boom)))
  (ck-probe (lambda ()
              (list 'a ((lambda (z) (list 'b z (abort-current-continuation t-ck 'ab))) 'zz)))
            (lambda (v k)
              (list 'handler (with-exception-handler k (lambda () (raise-continuable 'boom)))))))

;; A `dynamic-wind` extent inside the captured region is entered again by the
;; invoke — composable invokes do not travel, so this happens whether or not
;; the live stack already shares the extent — and left when the resumed
;; computation returns through it.
(test-equal "an extent inside the capture is re-entered and left by the invoke"
  '((handler ab (got 10)) (in out in out))
  (let ((log '()))
    (let* ((r (ck-probe (lambda ()
                          (dynamic-wind
                            (lambda () (set! log (cons 'in log)))
                            (lambda () (list 'got (abort-current-continuation t-ck 'ab)))
                            (lambda () (set! log (cons 'out log)))))
                        (lambda (v k) (list 'handler v (k 10)))))
           (l (reverse log)))
      (list r l))))

;; ─ Transfers carry the dynamic environment (Racket; Gauche for the handlers)
;;
;; The abort was unwinding four of the five components and the delimited
;; capture was saving three:
;;
;;                                            fixed            main (62a47df9)
;;   abort uninstalls the region's handlers   ESCAPED-TO-TOP   INNER
;;   composable k carries its handler stack   (got (INNER …))  unhandled exception
;;   composable k carries an inner prompt     (outer (inner…)) no matching prompt tag
;;
;; Racket answers the abort and inner-prompt rows directly, and Gauche answers
;; the handler row through `gauche.partcont`'s `shift`/`reset` with real
;; `with-exception-handler` and `raise-continuable` — which matters, because
;; Racket's handlers *are* continuation marks, so Racket alone could not tell
;; "carries the dynamic environment" from an artifact of that representation.
;;
;; Why the boundary is a recorded depth and not a frame depth: the first
;; version of the fix inferred it, and the first two rows are the shapes that
;; killed that. A `with-exception-handler` in tail position of the prompt body
;; installs at the prompt's own frame depth, because the body frame is already
;; popped — and so does a handler whose thunk tail-calls
;; `call-with-continuation-prompt`. One is inside the prompt and must go, the
;; other encloses it and must stay, and by depth they are indistinguishable.

(define t-env (make-continuation-prompt-tag 'environment))

;; The abort leaves the extents it unwinds, so the handler installed inside
;; the prompt body must not catch a raise made from the prompt handler. The
;; enclosing handler reaches the prompt by a *tail* call, so it shares the
;; prompt's frame depth and must survive anyway.
(test-equal "an abort uninstalls the handlers of the region it abandons"
  '(ESCAPED-TO-TOP boom)
  (with-exception-handler
    (lambda (e) (list 'ESCAPED-TO-TOP e))
    (lambda ()
      (call-with-continuation-prompt
        (lambda ()
          (with-exception-handler
            (lambda (e) (list 'INNER e))
            (lambda () (list 'got (abort-current-continuation t-env 'ab)))))
        t-env
        (lambda (v k) (raise-continuable 'boom))))))

;; The mirror shape: the enclosing handler has a depth of its own, and the
;; *inner* one shares the prompt's. Same answer, for the opposite reason —
;; the row a depth test gets wrong in whichever direction the other gets
;; right.
(test-equal "and does so when it is the inner handler that shares the prompt's depth"
  '(r (TOP boom))
  (with-exception-handler
    (lambda (e) (list 'TOP e))
    (lambda ()
      (list 'r
        (call-with-continuation-prompt
          (lambda ()
            (with-exception-handler
              (lambda (e) (list 'INNER e))
              (lambda () (list 'got (abort-current-continuation t-env 'ab)))))
          t-env
          (lambda (v k) (raise-continuable 'boom)))))))

;; A composable continuation carries the handlers it was captured under.
;; Escaping it out of the prompt first is what makes this independent of the
;; rows above: the handler the abort used to leak has been swept before the
;; resume, so a wrong answer here cannot be rescued by a wrong answer there.
;; The two used to mask each other. And they do not outlive the resumed
;; frames: the sweep that pops any other handler pops these, because their
;; recorded depths were relocated with the frames.
(test-equal "a composable continuation carries its handlers, and only while it runs"
  '(captured (got (INNER boom)) (OUTER again))
  (let* ((k* #f)
         (captured (call-with-continuation-prompt
                     (lambda ()
                       (with-exception-handler
                         (lambda (e) (list 'INNER e))
                         (lambda ()
                           (list 'got (raise-continuable
                                        (abort-current-continuation t-env 'ab))))))
                     t-env
                     (lambda (v k) (set! k* k) 'captured)))
         (resumed (k* 'boom))
         (after (with-exception-handler
                  (lambda (e) (list 'OUTER e))
                  (lambda () (raise-continuable 'again)))))
    (list captured resumed after)))

;; And it carries a prompt established inside the captured region: the
;; resumed frames abort to a delimiter their own code set up.
(define u-env (make-continuation-prompt-tag 'environment-inner))
(test-equal "a composable continuation carries a prompt established inside it"
  '(handler ab (outer (inner-handler ab2)))
  (call-with-continuation-prompt
    (lambda ()
      (list 'outer
        (call-with-continuation-prompt
          (lambda () (list 'inner (abort-current-continuation t-env 'ab)
                                  (abort-current-continuation u-env 'ab2)))
          u-env
          (lambda (v2 k2) (list 'inner-handler v2)))))
    t-env
    (lambda (v k) (list 'handler v (k 10)))))

;; ─ A composable continuation relocates the depths it carries (Guile)
;;
;; The rows above pin the *semantics*, but every one resumes at the depths it
;; was captured at, so every relocation in the invoke cancels to zero and they
;; pass with the arithmetic deleted. These do not. A `PromptFrame` records a
;; position in **three** stacks — frames, winds, handlers — and a carried
;; prompt needs all three moved onto the live ones. Relocating only the frame
;; depth gave, in order below: an enclosing after thunk run early, handlers
;; enclosing the invoke uninstalled, and a panic. The last two rows cover the
;; abort itself: its handler truncation runs before the after thunks rather
;; than after (#162, in the one window the first fix missed), and a capture
;; whose recorded depth has outrun the live stack clamps instead of slicing.
;;
;; Guile is the oracle rather than Racket: `(ice-9 control)`'s
;; `call-with-prompt`/`abort-to-prompt` are tagged like Patina's *and* it has
;; R7RS `with-exception-handler` and `raise-continuable`, so all five programs
;; transcribe one for one. It agrees on every row. The tree-walker's carried
;; prompts record two depths rather than three (a CPS continuation is not a
;; stack) and relocate them the same way; these rows hold it to the same
;; answers.

(define t-rel (make-continuation-prompt-tag 'relocate))
(define u-rel (make-continuation-prompt-tag 'relocate-inner))
(define k-rel #f)
;; Capture a continuation whose region contains an inner prompt, so the
;; carried `PromptFrame` has depths of its own to relocate.
(define (capture-rel!)
  (call-with-continuation-prompt
    (lambda ()
      (list 'outer
        (call-with-continuation-prompt
          (lambda () (list 'inner (abort-current-continuation t-rel 'ab)
                                  (abort-current-continuation u-rel 'ab2)))
          u-rel (lambda (v2 k2) (list 'inner-handler v2)))))
    t-rel (lambda (v k) (set! k-rel k) 'captured)))

;; Resumed inside a `dynamic-wind`: the abort to the carried prompt must
;; truncate the live wind stack at *its* extent, not at the depth it recorded
;; against another stack. Unrelocated, OUT ran twice.
(test-equal "resumed inside a dynamic-wind, the carried prompt truncates at its own extent"
  '((outer (inner-handler ab2)) (IN OUT))
  (let ((log '()))
    (capture-rel!)
    (let* ((r (dynamic-wind (lambda () (set! log (cons 'IN log)))
                            (lambda () (k-rel 10))
                            (lambda () (set! log (cons 'OUT log)))))
           (l (reverse log)))
      (list r l))))

;; Resumed under a handler: the abort to the carried prompt must not
;; uninstall handlers that enclose the invoke. Unrelocated, the raise after it
;; was unhandled.
(test-equal "resumed under a handler, the carried prompt leaves it installed"
  '((outer (inner-handler ab2)) (H ping))
  (begin
    (capture-rel!)
    (with-exception-handler
      (lambda (e) (list 'H e))
      (lambda () (let ((r (k-rel 10))) (list r (raise-continuable 'ping)))))))

;; Captured inside a `dynamic-wind` and resumed outside every one: the
;; carried prompt records a wind depth the live stack cannot reach, and
;; slicing at it panicked.
(test-equal "captured inside a dynamic-wind, resumed outside it"
  '(captured (outer (inner-handler ab2)))
  (let ((captured (dynamic-wind (lambda () #f)
                                (lambda () (capture-rel!))
                                (lambda () #f))))
    (list captured (k-rel 10))))

;; A raise from an after thunk the abort itself runs. The handler installed
;; inside the region being abandoned must already be gone — the answer the
;; `call/cc` path gives for the same shape, because a jump runs each thunk
;; under its own record's handler stack.
(test-equal "an abort's after thunk does not see the handlers it abandoned"
  '((prompt-handler ab) ((TOP from-after)))
  (let ((log '()))
    (let* ((r (with-exception-handler
                (lambda (e) (set! log (cons (list 'TOP e) log)) 'top)
                (lambda ()
                  (call-with-continuation-prompt
                    (lambda ()
                      (dynamic-wind
                        (lambda () #f)
                        (lambda ()
                          (with-exception-handler
                            (lambda (e) (set! log (cons (list 'INNER e) log)) 'inner)
                            (lambda () (abort-current-continuation t-rel 'ab))))
                        (lambda () (raise-continuable 'from-after))))
                    t-rel
                    (lambda (v k) (list 'prompt-handler v))))))
           (l (reverse log)))
      (list r l))))

;; A handler aborting to a prompt established inside its own thunk: the raise
;; has already popped the entry it is running, so the live handler stack is
;; shorter than the prompt recorded. Slicing at the recorded length panicked.
(test-equal "a handler may abort to a prompt inside its own thunk"
  '(handler (aborted x))
  (with-exception-handler
    (lambda (e) (abort-current-continuation t-rel (list 'aborted e)))
    (lambda ()
      (call-with-continuation-prompt
        (lambda () (raise-continuable 'x))
        t-rel
        (lambda (v k) (list 'handler v))))))

;; ─ An abort's after thunks run as frames (Guile)
;;
;; A jump has run each wind thunk under its own record's handler stack, in a
;; frame of its own, since #156; the value form of `dynamic-wind` since #158.
;; The abort ran its after thunks on a nested Rust loop instead:
;;
;;                                   fixed               main (7ebfed1f)
;;   after thunk's raise, handler    (MID from-after)    (TOP from-after)
;;     captured by the record
;;   re-enter an after thunk         after-1 once,       after-1 twice,
;;                                   value kept          value lost
;;
;; It is a jump now: it travels to a continuation whose top frame calls the
;; prompt handler, because leaving every extent between here and a target is
;; what a travel *is*. A composable invoke's re-entry thunks deliberately do
;; **not** go through the travel — the next group pins why.

(define t-aft (make-continuation-prompt-tag 'after-thunks))

;; The handler sits *between* the prompt and the `dynamic-wind`, so the
;; record captured it and its after thunk must run under it. The same program
;; left by a jump instead of an abort answers MID on both backends, chibi,
;; Gauche and Guile.
(test-equal "an abort's after thunk runs under the handler its record captured"
  '((prompt-handler ab) ((MID from-after)))
  (let ((log '()))
    (let* ((r (with-exception-handler
                (lambda (e) (set! log (cons (list 'TOP e) log)) 'top)
                (lambda ()
                  (call-with-continuation-prompt
                    (lambda ()
                      (with-exception-handler
                        (lambda (e) (set! log (cons (list 'MID e) log)) 'mid)
                        (lambda ()
                          (dynamic-wind
                            (lambda () #f)
                            (lambda () (abort-current-continuation t-aft 'ab))
                            (lambda () (raise-continuable 'from-after))))))
                    t-aft
                    (lambda (v k) (list 'prompt-handler v))))))
           (l (reverse log)))
      (list r l))))

;; Re-entering a continuation captured inside an after thunk the abort is
;; running resumes *after* the capture — after-1 once — and the abort still
;; delivers its value. On `main` the thunk restarted from the top and `r`
;; came out #<unspecified>: #157's signature, on the abort.
(test-equal "re-entering an abort's after thunk resumes it and keeps the abort's value"
  '((prompt-handler ab) (before after-1 after-2 after-2))
  (let ((k #f) (n 0) (log '()))
    (let ((r (call-with-continuation-prompt
               (lambda ()
                 (dynamic-wind (lambda () (set! log (cons 'before log)))
                               (lambda () (abort-current-continuation t-aft 'ab))
                               (lambda ()
                                 (set! log (cons 'after-1 log))
                                 (call/cc (lambda (c) (set! k c)))
                                 (set! log (cons 'after-2 log)))))
               t-aft (lambda (v k2) (list 'prompt-handler v)))))
      (if (< n 1) (begin (set! n 1) (k 'again)))
      (list r (reverse log)))))

;; Escaping *out* of an after thunk the abort is running already worked and
;; has to keep working: the travel reports the escape like any other, rather
;; than swallowing it.
(test-equal "escaping out of an abort's after thunk still escapes"
  'escaped-from-after
  (let ((esc #f))
    (let ((r (call/cc (lambda (c) (set! esc c) 'first))))
      (if (eq? r 'first)
          (call-with-continuation-prompt
            (lambda ()
              (dynamic-wind (lambda () #f)
                            (lambda () (abort-current-continuation t-aft 'ab))
                            (lambda () (esc 'escaped-from-after))))
            t-aft (lambda (v k) (list 'prompt-handler v))))
      r)))

;; ─ A composable invoke's re-entry thunks (Guile)
;;
;; They run under the **invoke site's** handler stack, which is why the
;; abort's fix does not generalise. Routing the abort through the travel is
;; right: its target *replaces* the machine, so installing the record's own
;; captured stack is exactly R7RS 6.10. A composable invoke's target would
;; *extend* the machine, and there the same call is wrong — the invoke site's
;; handlers disappear and capture-site ones whose extent is long over come
;; back. That shipped for one review cycle; under the travel the first row
;; died with `unhandled exception: boom-in` and the second answered
;; CAPTURE-SITE. The thunks *are* frames now — issue #167 gave them a stub of
;; their own — which is a different thing from routing them through the
;; travel, and the reason it is a separate mechanism.

(define t-re (make-continuation-prompt-tag 're-entry))
(define t2-re (make-continuation-prompt-tag 're-entry-outer))

;; A `guard` around the invoke must see a raise from the re-entered extent's
;; before thunk.
(test-equal "a re-entry before thunk's raise reaches the guard around the invoke"
  '(cap guarded (in out in (G boom-in)))
  (let* ((log '())
         (note (lambda (x) (set! log (cons x log))))
         (k* #f)
         (cap (call-with-continuation-prompt
                (lambda ()
                  (dynamic-wind (lambda () (note 'in) (if k* (raise 'boom-in)))
                                (lambda () (list 'got (abort-current-continuation t-re 'ab)))
                                (lambda () (note 'out))))
                t-re (lambda (v k) (set! k* k) 'cap)))
         (r (guard (e (#t (note (list 'G e)) 'guarded)) (k* 5))))
    (list cap r (reverse log))))

;; And a handler at the invoke site wins over one whose extent ended when the
;; continuation was captured.
(test-equal "a re-entry thunk runs under the invoke site's handler"
  '(cap (got 5) INVOKE-SITE)
  (let* ((k* #f)
         (seen #f)
         (cap (with-exception-handler
                (lambda (e) (set! seen 'CAPTURE-SITE) 'c)
                (lambda ()
                  (call-with-continuation-prompt
                    (lambda ()
                      (dynamic-wind (lambda () (if k* (raise-continuable 'boom-in)))
                                    (lambda () (list 'got (abort-current-continuation t-re 'ab)))
                                    (lambda () #f)))
                    t-re (lambda (v k) (set! k* k) 'cap)))))
         (r (with-exception-handler
              (lambda (e) (set! seen 'INVOKE-SITE) 'i)
              (lambda () (k* 5)))))
    (list cap r seen)))

;; Two shapes the same change fixed that `control_flow_matrix.rs`'s axes do
;; not reach — a *transfer out of* a re-entry thunk, and *nested* captured
;; extents. Guile answers both as here.
;;
;;                              fixed                      main (a18d7ae1)
;;   abort out of a re-entry    (outer esc), (in out in)   swallowed: (got 5),
;;     before thunk                                        (in out in out)
;;   nested captured extents,   (got 5), B re-entered      value lost, B never
;;     inner thunk jumps out                               re-entered
;;
;; The re-entered extent's before thunk aborts to a prompt outside the invoke.
;; The abort must win — the resumed computation never runs, and the extent it
;; was entering is left without running its after thunk, because a before
;; thunk that does not return never entered.
(test-equal "an abort out of a re-entry before thunk wins"
  '(cap (outer esc) (in out in))
  (let* ((log '())
         (note (lambda (x) (set! log (cons x log))))
         (k* #f)
         (cap (call-with-continuation-prompt
                (lambda ()
                  (dynamic-wind
                    (lambda () (note 'in) (if k* (abort-current-continuation t2-re 'esc) #f))
                    (lambda () (list 'got (abort-current-continuation t-re 'ab)))
                    (lambda () (note 'out))))
                t-re (lambda (v k) (set! k* k) 'cap)))
         (r (call-with-continuation-prompt
              (lambda () (k* 5)) t2-re (lambda (v k) (list 'outer v)))))
    (list cap r (reverse log))))

;; Two nested captured extents, with the *outer* before thunk capturing a
;; continuation that is re-entered later. Resuming it has to finish entering
;; the inner extent too — on `main` the inner one was never re-entered and the
;; resumed value was lost.
(test-equal "re-entering nested captured extents finishes entering both"
  '(cap (got 5) (A-in1 A-in2 B-in B-out A-out A-in1 A-in2 B-in B-out A-out A-in2 B-in B-out A-out))
  (let* ((log '())
         (note (lambda (x) (set! log (cons x log))))
         (k* #f)
         (kt #f)
         (n 0)
         (cap (call-with-continuation-prompt
                (lambda ()
                  (dynamic-wind
                    (lambda () (note 'A-in1) (call/cc (lambda (c) (set! kt c))) (note 'A-in2))
                    (lambda () (dynamic-wind (lambda () (note 'B-in))
                                             (lambda () (list 'got (abort-current-continuation t-re 'ab)))
                                             (lambda () (note 'B-out))))
                    (lambda () (note 'A-out))))
                t-re (lambda (v k) (set! k* k) 'cap))))
    (let ((r (k* 5)))
      (when (< n 1) (set! n 1) (kt 'again))
      (list cap r (reverse log)))))

;; ─ The remainder of a raise is a frame (issue #178; Guile)
;;
;; So a continuation captured inside the handler carries it. The rows that
;; changed when this landed are elsewhere in this file and in
;; `cps-features.scm`; what is here is the property none of them pins: **when
;; the reinstated handler goes away again.** R7RS 6.11 puts it back for the
;; rest of the thunk's extent, so a replay of that debt has to end where the
;; replayed region does — otherwise a handler answers a raise long after its
;; `with-exception-handler` returned, and which one answers depends on frame
;; counts nobody wrote down. The VM gives the entry a depth measured from the
;; floor of the smallest region that could replay the frame; the tree-walker
;; sweeps back to the prompt frame's `handler_depth` at the boundary.
;;
;; Finding programs that can see this is most of the work, and the ones that
;; cannot are worth naming, because each looks like it tests the property:
;; `(raise-continuable (abort-current-continuation …))` captures at the abort,
;; which runs first, so no raise frame is in the region; resuming at the
;; capture's depth makes every relocation cancel; a `guard` around the later
;; raise installs its own handler on top and answers either way; and invoking
;; the continuation twice while reading only the two values cannot tell a
;; handler pushed per invoke and never popped. The trailing raise in each row
;; is what makes it bite.

;; `k*` resumes a body whose handler aborted, with `nest` frames between the
;; `with-exception-handler` and the prompt. INSIDE puts the handler within
;; the captured region, OUTSIDE beyond the region's floor — the case whose
;; depth no region contains. Each call builds its own tag and continuation.
(define (raise-remainder nest inside use)
  (let* ((t (make-continuation-prompt-tag 'raise-remainder))
         (k* #f)
         (handler (lambda (e) (if (eq? e 'rc)
                                  (abort-current-continuation t 'ab)
                                  (list 'INNER e))))
         (nested (lambda (n thunk)
                   (let loop ((n n))
                     (if (= n 0) (thunk) (list 'L (loop (- n 1)))))))
         (body (lambda ()
                 (call-with-continuation-prompt
                   (lambda () (list 'body (raise-continuable 'rc)))
                   t (lambda (v k) (set! k* k) 'cap))))
         (cap (if inside
                  (call-with-continuation-prompt
                    (lambda () (with-exception-handler handler
                                 (lambda () (list 'body (raise-continuable 'rc)))))
                    t (lambda (v k) (set! k* k) 'cap))
                  (with-exception-handler handler (lambda () (nested nest body))))))
    (use k* nested)))

;; The handler is installed *inside* the prompt, so the region contains its
;; extent. Resuming replays the re-push; once the resumed body has returned,
;; the next raise must reach OUTER. Nesting the resume puts the replay at a
;; depth the capture never saw.
(test-equal "a replayed handler goes away with the region that replayed it"
  '(L (L (L (L (L (L ((body resumed) (OUTER after))))))))
  (raise-remainder 0 #t
    (lambda (k* nested)
      (with-exception-handler (lambda (e) (list 'OUTER e))
        (lambda ()
          (nested 6 (lambda ()
                      (let ((r (k* 'resumed))) (list r (raise-continuable 'after))))))))))

;; The handler is installed *outside* the prompt, so its own depth names a
;; frame below anything the region owns. Reinstating there on a replay put
;; the entry where nothing sweeps it, and the answer flipped to INNER as soon
;; as one frame separated the handler from the prompt — the VM was right at 0
;; and wrong from 1, the tree-walker wrong at every count. Guile answers OUTER
;; throughout.
(test-equal "a handler outside the region is not reinstated by a replay, at any depth"
  '(((body resumed) (OUTER after)) ((body resumed) (OUTER after))
    ((body resumed) (OUTER after)) ((body resumed) (OUTER after))
    ((body resumed) (OUTER after)))
  (map (lambda (n)
         (raise-remainder n #f
           (lambda (k* nested)
             (with-exception-handler (lambda (e) (list 'OUTER e))
               (lambda () (let ((r (k* 'resumed))) (list r (raise-continuable 'after))))))))
       '(0 1 2 5 20)))

;; Invoking the same continuation twice reinstalls once per resumption and
;; leaves nothing behind: the trailing raise reaches OUTER, which it would not
;; if the two re-pushes had accumulated.
(test-equal "two resumptions reinstall once each and leave nothing behind"
  '((body one) (body two) (OUTER after))
  (raise-remainder 0 #t
    (lambda (k* nested)
      (with-exception-handler (lambda (e) (list 'OUTER e))
        (lambda () (let* ((a (k* 'one)) (b (k* 'two)))
                     (list a b (raise-continuable 'after))))))))

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

;; An abort out of a callback reached through a *nested trampoline* — the
;; one a Rust primitive's callback runs on, on the tree-walker — finds its
;; prompt. Both backends since 2026-09-10; the VM since #177. The
;; tree-walker's callback trampoline used to start with every stack empty,
;; so the abort searched a prompt stack holding none of the caller's prompts
;; and raised "no matching prompt tag", which the wrapper `guard` then
;; caught: (caught #t). The callback now runs under the caller's stacks, and
;; the landing belongs to the prompt's own trampoline, so the abort unwinds
;; through `assoc` to it (`cps_eval/prompts.rs`).
;;
;; `force` is not the probe here: the tree-walker routes it through the CPS
;; evaluator rather than the trampoline, so it answered (h a1) throughout. A
;; comparator passed to `assoc` does go through the trampoline.
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

;; The prompts a composable continuation carries are re-established on
;; whatever run invokes it. Both prompts here are pushed *inside* a
;; callback; the abort to the outer one hands its handler `kk`, whose region
;; still holds the inner prompt; `kk` is invoked after `member` has
;; returned, and the abort inside the resumed region must find that inner
;; prompt on the *current* run. The first cut of the tree-walker fix copied
;; the capture-site trampoline onto the relocated frame and refused the
;; abort as belonging to a run that had ended.
(define t-reloc-outer (make-continuation-prompt-tag 'reloc-outer))
(define t-reloc-inner (make-continuation-prompt-tag 'reloc-inner))
(test-equal "a carried prompt is re-established on the invoking run"
  '((1) (inner (resumed-with 42)))
  (let ((kk #f))
    (let ((first (member 1 '(1) (lambda (a b)
                   (call-with-continuation-prompt
                     (lambda ()
                       (call-with-continuation-prompt
                         (lambda ()
                           (+ 100 (let ((v (abort-current-continuation t-reloc-outer 'up)))
                                    (abort-current-continuation t-reloc-inner
                                                                (list 'resumed-with v)))))
                         t-reloc-inner (lambda (v k) (list 'inner v))))
                     t-reloc-outer (lambda (v k) (set! kk k) v))))))
      (list first (kk 42)))))

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
  (let* ((r (call-with-continuation-prompt (make-parameter 9) t-179 (lambda (v k) 'h)))
         (after (guard (e (#t 'no-prompt)) (abort-current-continuation t-179 'stale))))
    (list r after)))

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
  (let* ((r (call-with-continuation-prompt values t-186 (lambda (v k) 'h) 5))
         (after (guard (e (#t 'no-prompt)) (abort-current-continuation t-186 'stale))))
    (list r after)))

;; A body that aborts to the prompt this very call pushed.
(test-equal "a control primitive can be the prompt body: abort-current-continuation itself"
  '(h ab)
  (call-with-continuation-prompt abort-current-continuation t-186
    (lambda (v k) (list 'h v)) t-186 'ab))

;; ── guard, resumed through a composable continuation ───────────────────────
;;
;; `lib/scheme/base/exceptions.scm` defines `guard` by R7RS 7.3's expansion,
;; which captures `guard-k` — a *full* continuation — when the `guard` is
;; entered, and leaves through it. If a composable continuation captured the
;; guard's body and is resumed from somewhere else, a jump to `guard-k` goes
;; back to the *original* context rather than returning to the invoker.
;; Racket 9.3 (`with-handlers`) and Guile 3.0.11 (`--r7rs`'s `guard`) both
;; return to the invoker, for both rows below.
;;
;; Each row keeps the whole scenario inside its own expression, and resumes
;; `kk` at most once: a wrong jump then re-runs the row's own `let` and
;; delivers a wrong value to the row, instead of re-running an earlier
;; top-level form and taking the row with it.

(define t-guard (make-continuation-prompt-tag 'guard))

;; The success path. R7RS 7.3's reference line jumps to `guard-k` here as
;; well; ours returns, and this row is why. With the reference line — tried
;; and measured 2026-09-11 — both backends answered `((r x))`, the resumed
;; `(list 1 _)` lost to a jump back to the first evaluation.
(test-equal "a composable continuation resumed through a successful guard returns to its invoker"
  '((r x) (resumed (1 10)))
  (let ((kk #f) (log '()))
    (let ((r (call-with-continuation-prompt
               (lambda () (list 1 (guard (e (#t 'caught))
                                    (abort-current-continuation t-guard 'x)
                                    10)))
               t-guard (lambda (v k) (set! kk k) v))))
      (set! log (cons (list 'r r) log))
      (if (= (length log) 1)
          (set! log (cons (list 'resumed (kk 'resumed)) log)))
      (reverse log))))

;; The raise path, where the call/cc expansion has no alternative: a clause's
;; result goes out through `guard-k` whichever way the body left. Both
;; backends answer `((r x) (r (1 caught)))` — the resumed computation's
;; value arrives at the *first* evaluation's `r`, and the resume never
;; returns. An expected failure on both backends, so it announces itself
;; when a prompt-based `guard` lands (Track L §6).
(cond-expand (patina (test-expect-fail 1)) (else))
(test-equal "a composable continuation resumed through a guard whose body raises returns to its invoker"
  '((r x) (resumed (1 caught)))
  (let ((kk #f) (log '()))
    (let ((r (call-with-continuation-prompt
               (lambda () (list 1 (guard (e (#t 'caught))
                                    (abort-current-continuation t-guard 'x)
                                    (raise 'boom))))
               t-guard (lambda (v k) (set! kk k) v))))
      (set! log (cons (list 'r r) log))
      (if (= (length log) 1)
          (set! log (cons (list 'resumed (kk 'resumed)) log)))
      (reverse log))))

(test-end)
