;; Parameter objects: `make-parameter`, `parameterize`, and what follows from
;; R7RS §4.2.6 making a parameter a procedure.
;;
;; Migrated from `crates/patina-tests/tests/parameters.rs` (#193 Phase 1).
;; Three assertions could not come along, and are named where they went:
;;
;;   - the two malformed-`parameterize` programs, which assert that an
;;     *unguarded* program fails — `callability.rs`, which already owns that
;;     class. The catchable half of the first is in this file, below;
;;   - that a parameter prints as `#<parameter>`, which is Patina's own
;;     external representation and not a claim another implementation can be
;;     held to — `external_representation.rs`.
;;
;; The predicate's own truth table lives with the other predicates in
;; `compliance/predicates.rs`; this file covers what being a procedure lets a
;; parameter *do*.

(import (scheme base) (srfi 64))

(test-begin "parameters")

;; ── Reading, setting, parameterize ──────────────────────────────────────────

(define p10 (make-parameter 10))
(test-equal "a parameter reads its initial value" 10 (p10))

(define p-set (make-parameter 10))
(test-equal "calling it with an argument sets it" 20
  (begin (p-set 20) (p-set)))

;; Bind and restore are one row, not two. Split apart, the restore half reads
;; `(test-equal 10 (p-simple))` — which also passes if `parameterize` never
;; bound anything at all, and so is only an assertion in the presence of the
;; row before it.
(define p-simple (make-parameter 10))
(test-equal "parameterize binds for the dynamic extent, and restores after" '(20 10)
  (let* ((inside (parameterize ((p-simple 20)) (p-simple)))
         (after (p-simple)))
    (list inside after)))

(define p-first (make-parameter 10))
(define p-second (make-parameter 20))
(test-equal "several parameters at once" '(100 200)
  (parameterize ((p-first 100) (p-second 200)) (list (p-first) (p-second))))

(define p-nested (make-parameter 10))
(test-equal "nested parameterize takes the inner value" 30
  (parameterize ((p-nested 20)) (parameterize ((p-nested 30)) (p-nested))))
(test-equal "…and unwinding restores the outer one" 20
  (parameterize ((p-nested 20)) (parameterize ((p-nested 30)) (p-nested)) (p-nested)))

(define p-body (make-parameter 10))
(test-equal "a multi-expression body yields its last value" 20
  (parameterize ((p-body 20)) (p-body) (p-body) (p-body)))

;; A non-parameter in the binding position raises, and the raise is catchable.
;; The *unguarded* half — that the same program fails a top-level program with
;; nothing to catch it — is `callability.rs`, because `test-error` runs its body
;; inside `call/cc` and `with-exception-handler` and so cannot state it.
(test-error "a non-parameter in the binding position is an error" #t
  (parameterize ((42 20)) 'body))

;; ── Converters (R7RS §4.2.6) ────────────────────────────────────────────────

;; A parameter apiece, so that neither row depends on the other having run:
;; the second one *sets*, and setting the parameter the first row reads would
;; make the pair order-dependent for no gain.
(define p-conv (make-parameter 10 (lambda (x) (* x 2))))
(test-equal "the converter is applied to the initial value" 20 (p-conv))

(define p-conv-set (make-parameter 10 (lambda (x) (* x 2))))
(test-equal "…and to a value assigned later" 10
  (begin (p-conv-set 5) (p-conv-set)))

;; ── A parameter is a procedure (R7RS §4.2.6) ────────────────────────────────

;; The predicate and the call path must agree: anything `procedure?` accepts
;; should be callable through the ordinary procedure routes. A parameter has its
;; own calling convention internally, which is why this is worth asserting
;; rather than assuming. Both routes worked before `procedure?` was fixed — only
;; the predicate lied — so this documents the contrast rather than guarding the
;; fix.
(define p7 (make-parameter 7))
(test-equal "callable the ways a procedure is" '(7 (7))
  (list (apply p7 '()) (map (lambda (f) (f)) (list p7))))

;; Accepted where a procedure is *required* — the places that actually gate on
;; the predicate, both of which rejected a parameter before the fix.
;; `dynamic-wind` is deliberately absent: it type-checks nothing on either
;; backend and simply calls its thunks, so it accepted a parameter all along and
;; would pin nothing.
(test-equal "accepted as an exception handler" 'ran
  (with-exception-handler (make-parameter 1) (lambda () 'ran)))

;; `make-parameter`'s converter check is the line the fix edited: it used to
;; read `is_procedure(c) || is_parameter(c)` and now relies on `is_procedure`
;; alone.
;;
;; `converter-target` is itself a parameter, used here as another parameter's
;; converter. Constructing `p-with-parameter-converter` calls it with 5, which
;; *sets* it — so the evidence that the converter was accepted **and applied**
;; is in the target, not in the parameter being constructed, which nothing ever
;; reads. The row names it anyway, to say out loud that it is what makes the
;; claim true.
(define converter-target (make-parameter 0))
(define p-with-parameter-converter (make-parameter 5 converter-target))
(test-equal "accepted as a converter" 5
  (begin p-with-parameter-converter (converter-target)))

;; ── Convert once, before the wind (R7RS §4.2.6) ─────────────────────────────

;; D2 — the restore must not run the converter again. `parameterize` used to
;; restore by *calling* the parameter, and calling a parameter converts, so the
;; old value went through the converter a second time on the way out.
;;
;; Sequenced with `let*` rather than read in argument positions: R7RS leaves
;; argument order unspecified, and these reads are order-dependent by
;; construction.
(define p-restore (make-parameter 10 (lambda (x) (* x 2))))
(test-equal "the converter does not run again on restore" '(20 2 20)
  (let* ((before (p-restore))
         (inside (parameterize ((p-restore 1)) (p-restore)))
         (after (p-restore)))
    (list before inside after)))

;; A type-changing converter makes the second application *raise*, so a
;; regression here fails the restore outright rather than producing a wrong
;; value.
(define p-typed (make-parameter 1 number->string))
(test-equal "a type-changing converter survives the restore" '("2" "1")
  (let* ((inside (parameterize ((p-typed 2)) (p-typed)))
         (after (p-typed)))
    (list inside after)))

;; D1 — a `parameterize` that fails partway leaves nothing bound. The bindings
;; used to be installed inside `dynamic-wind`'s *before* thunk, so a later one
;; raising meant the after thunk never ran and the earlier ones stayed installed
;; for good.
(define p-unchanged (make-parameter 'a0))
(define p-rejecting (make-parameter 'b0 (lambda (v) (if (eq? v 'bad) (error "no") v))))
(test-equal "a converter that raises leaves no binding changed" '(caught a0 b0)
  (let* ((caught (guard (e (#t 'caught))
                   (parameterize ((p-unchanged 'a1) (p-rejecting 'bad)) 'body)))
         (av (p-unchanged))
         (bv (p-rejecting)))
    (list caught av bv)))

;; The same property when the *install* raises rather than the converter. This
;; is the failure the standard ports have, since they are procedures over a
;; thread-local that validate on assignment. #70 put every program write behind
;; it: a leak here sent all later output to `sink` for the rest of the run.
;;
;; **Scoped to Patina, because its premise is.** Whether assigning a non-port to
;; `current-input-port` raises at all is implementation-specific — chibi accepts
;; it, so the `guard` never fires there and the row would report a difference
;; that is about port validation, not about `parameterize`. The property itself
;; is portable and the converter row above covers it; this one covers the path
;; where the *install* is what fails. `write-string` rather than `display`
;; because `(scheme base)` exports it and `(scheme write)` is where `display`
;; lives — chibi warns about the undefined reference even though the row never
;; runs there, which is how this file's import set stays honest.
;;
;; Elsewhere it is a **reported skip**, not a silent absence: `cond-expand`
;; alone would delete the row with nothing anywhere saying so, and a row that
;; can vanish quietly is the failure mode this suite's skip and floor checks
;; exist to prevent.
(define sink (open-output-string))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an install that raises leaves no binding changed" '(caught 0)
  (let* ((caught (guard (e (#t 'caught))
                   (parameterize ((current-output-port sink)
                                  (current-input-port 5))
                     'body)))
         (_ (write-string "visible"))
         (leaked (string-length (get-output-string sink))))
    (list caught leaked)))

;; The converter runs once per `parameterize`, not once per entry — what
;; converting outside the wind buys beyond the two bugs above. A continuation
;; that re-enters the body re-installs the value; it must not re-convert it.
;; One conversion for `(make-parameter 0 …)`, one for `(p-count 1)`.
(define conversions 0)
(define p-count (make-parameter 0 (lambda (v) (set! conversions (+ conversions 1)) v)))
(define reenter-k #f)
(define entries 0)
(test-equal "the converter runs once per parameterize, not per entry" '(2 2)
  (begin
    (parameterize ((p-count 1))
      (call/cc (lambda (c) (set! reenter-k c)))
      (set! entries (+ entries 1))
      (if (< entries 2) (reenter-k #f)))
    (list entries conversions)))

(test-end)
