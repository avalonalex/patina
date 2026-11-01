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
;;     held to — `data/external-representation.scm`, scoped to Patina there.
;;
;; The predicate's own truth table lives with the other predicates in
;; `data/predicates.scm`; this file covers what being a procedure lets a
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

;; The same property when the port is the one rejected. This was the failure
;; the standard ports had when they were procedures that validated on
;; assignment, so the *install* raised: #70 put every program write behind
;; it, and a leak here sent all later output to `sink` for the rest of the
;; run. Since #608 they are parameter objects whose converters check the
;; port, so this row raises in `current-input-port`'s converter, before
;; anything is installed; the row after it keeps the install's undo covered.
;;
;; **Scoped to Patina, because its premise is.** Whether assigning a non-port to
;; `current-input-port` raises at all is implementation-specific — chibi accepts
;; it, so the `guard` never fires there and the row would report a difference
;; that is about port validation, not about `parameterize`. The property itself
;; is portable, and the converter row above covers it; this one holds it for
;; the standard ports, whose converters are port checks. `write-string` rather than `display`
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
(test-equal "a rejected port leaves no binding changed" '(caught 0)
  (let* ((caught (guard (e (#t 'caught))
                   (parameterize ((current-output-port sink)
                                  (current-input-port 5))
                     'body)))
         (_ (write-string "visible"))
         (leaked (string-length (get-output-string sink))))
    (list caught leaked)))

;; The install's own undo, which the row above stopped reaching when the
;; standard ports became parameter objects (#608): `parameterize` over two
;; parameter-like procedures a program wrote, which take a value on
;; assignment and have no converter, the second raising as it is assigned.
;; The first is put back before the error escapes. The undo is Scheme's,
;; `%parameterize-each` in `patina-primitives`' `parameterize_each.scm` (#784,
;; #817). Scoped to
;; Patina and reported as a skip elsewhere, as the row above is: R7RS makes
;; `parameterize` of anything but a parameter object an error, and chibi
;; raises; so are the rows after it, which hold the rest of that loop.
(define like-a-value 'a0)
(define (like-a . v) (if (null? v) like-a-value (set! like-a-value (car v))))
(define (like-b . v) (if (null? v) 'b0 (error "no")))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an install that raises puts back the ones before it" '(caught a0)
  (let* ((caught (guard (e (#t 'caught))
                   (parameterize ((like-a 'a1) (like-b 'b1)) 'body)))
         (av (like-a)))
    (list caught av)))

;; ── Procedures in the binding position, through Scheme (#784) ─────────────
;;
;; A list with a procedure a program wrote goes through `%parameterize-each`,
;; which reads each binding with `(p)` and sets it with `(p v)`, a call the
;; machine makes; a parameter object in the same list is installed raw. Each
;; row's procedures are its own: `make-setter` makes one over a box, which
;; answers it with no argument and sets it with one. Every row is scoped to
;; Patina, as the row above is.
(define (make-setter box) (lambda v (if (null? v) (car box) (set-car! box (car v)))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "a procedure in the binding position is set and restored" '(new old)
  (let* ((box (list 'old)) (q (make-setter box))
         (inside (parameterize ((q 'new)) (car box))))
    (list inside (car box))))

;; The parameter object is converted once, before the wind, and installed and
;; restored raw: two converter calls, one for `make-parameter`.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a mixed list converts its parameter object once" '((10 new) 2 old 2)
  (let* ((calls 0)
         (d (make-parameter 1 (lambda (x) (set! calls (+ calls 1)) (* x 2))))
         (box (list 'old))
         (q (make-setter box)))
    (let ((inside (parameterize ((d 5) (q 'new)) (list (d) (car box)))))
      (list inside (d) (car box) calls))))

;; #781: three earlier bindings, a parameter object among them, then a setter
;; that raises. Every one is put back, as the tree-walker and Gauche did; the
;; VM's Rust undo put back only the innermost under a `guard` or an escape.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a failed install puts back every earlier binding" '((caught rejected) old1 oldd old2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (p2 (make-setter b2))
         (d (make-parameter 'oldd))
         (bad (lambda v (if (null? v) 'old3 (raise 'rejected)))))
    (let ((caught (guard (e (#t (list 'caught e)))
                    (parameterize ((p1 'new1) (d 'newd) (p2 'new2) (bad 'new3)) 'body))))
      (list caught (car b1) (d) (car b2)))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "…and one that escapes does too" '(escaped old1 oldd old2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (p2 (make-setter b2))
         (d (make-parameter 'oldd)))
    (let ((r (call/cc
               (lambda (esc)
                 (let ((bad (lambda v (if (null? v) 'old3 (esc 'escaped)))))
                   (parameterize ((p1 'new1) (d 'newd) (p2 'new2) (bad 'new3)) 'body))))))
      (list r (car b1) (d) (car b2)))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "a setter that escapes leaves no binding changed" '(escaped old1 old2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)))
    (let ((r (call/cc
               (lambda (esc)
                 (let ((p2 (lambda v (if (null? v) (car b2) (esc 'escaped)))))
                   (parameterize ((p1 'new1) (p2 'new2)) 'body))))))
      (list r (car b1) (car b2)))))

;; A raise the setter's handler answers lets the install go on: the earlier
;; binding is put back while the outer handler runs, and installed again once
;; the setter returns.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a setter's answered raise lets the install go on" '((new1 (new2 42)) old1 (old2 42))
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2))
         (p2 (lambda v (if (null? v) (car b2) (set-car! b2 (list (car v) (raise-continuable 'ask)))))))
    (with-exception-handler
      (lambda (e) 42)
      (lambda ()
        (let ((inside (parameterize ((p1 'new1) (p2 'new2)) (list (car b1) (car b2)))))
          (list inside (car b1) (car b2)))))))

;; What the outer handler sees of the earlier binding, on the install and
;; then on the restore: the binding as it was before the setter ran. The Rust
;; undo left it installed while the handler ran, which answered (new1 old1).
(cond-expand (patina) (else (test-skip 1)))
(test-equal "the outer handler sees the earlier binding put back" '(old1 new1)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2))
         (p2 (lambda v (if (null? v) (car b2) (set-car! b2 (list (car v) (raise-continuable 'ask))))))
         (seen '()))
    (with-exception-handler
      (lambda (e) (set! seen (cons (car b1) seen)) 42)
      (lambda () (parameterize ((p1 'new1) (p2 'new2)) 'body)))
    (reverse seen)))

;; A `guard` whose clauses decline re-raises where the condition was raised
;; (R7RS §4.2.7), so it leaves the install and enters it again: it changes no
;; binding. The handler beyond it sees the earlier binding put back, on the
;; install and then on the restore, as in the handler's view above; when it
;; answers, the install goes on and the earlier binding is installed again.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a declining guard leaves an answered install as it was" '((new1 (new2 42)) old1 (old2 42) (old1 new1))
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (seen '())
         (p2 (lambda v (if (null? v) (car b2) (set-car! b2 (list (car v) (raise-continuable 'ask)))))))
    (with-exception-handler
      (lambda (e) (set! seen (cons (car b1) seen)) 42)
      (lambda ()
        (let ((inside (guard (e ((string? e) 'declined))
                        (parameterize ((p1 'new1) (p2 'new2)) (list (car b1) (car b2))))))
          (list inside (car b1) (car b2) (reverse seen)))))))

;; …and an answered restore: the earlier binding ends at its value before the
;; form, not at the inner value the restore's undo put back.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "…and an answered restore" '((new1 new2) old1 (old2 42))
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (restoring #f)
         (p2 (lambda v
               (cond ((null? v) (car b2))
                     (restoring (set-car! b2 (list (car v) (raise-continuable 'ask))))
                     (else (set-car! b2 (car v)))))))
    (with-exception-handler
      (lambda (e) 42)
      (lambda ()
        (let ((inside (guard (e ((string? e) 'declined))
                        (parameterize ((p1 'new1) (p2 'new2))
                          (set! restoring #t)
                          (list (car b1) (car b2))))))
          (list inside (car b1) (car b2)))))))

;; A setter that raises on the restore: the undo puts back the values the
;; restore was replacing, so both bindings keep their inner values.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a setter that raises on the restore leaves the inner values" '((caught no-restore) new1 new2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (restoring #f)
         (p2 (lambda v
               (cond ((null? v) (car b2))
                     (restoring (raise 'no-restore))
                     (else (set-car! b2 (car v)))))))
    (let ((caught (guard (e (#t (list 'caught e)))
                    (parameterize ((p1 'new1) (p2 'new2)) (set! restoring #t) 'body))))
      (list caught (car b1) (car b2)))))

;; A continuation captured in the setter, re-entered twice after the
;; `parameterize` returned, runs the rest of the install again, the body and
;; the restore. The setter used to be called from Rust, whose frame no
;; continuation can return into (#471): the VM raised "parameter and value
;; lists differ in length", and the tree-walker abandoned the form.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a continuation captured in a setter runs the install again"
  '(((new old) (new old) (new old)) (new old new old new old))
  (let ((held 'old) (log '()) (k #f) (results '()) (n 0))
    (define (q . v)
      (if (null? v)
          held
          (begin
            (call/cc (lambda (c) (if (not k) (set! k c))))
            (set! log (cons (car v) log))
            (set! held (car v)))))
    (let ((r (parameterize ((q 'new)) held)))
      (set! results (cons (list r held) results)))
    (if (< n 2) (begin (set! n (+ n 1)) (k #f)))
    (list (reverse results) (reverse log))))

;; The same, with an earlier binding, the program setting it after the form,
;; and the setter escaping when re-entered: the escape puts the earlier
;; binding back to its value before the form, as an exit before the setter
;; returns does on every entry.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a re-entered setter that escapes puts the earlier binding back" '(escaped old1 old2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (k #f) (out #f) (again #f)
         (p2 (lambda v
               (if (null? v)
                   (car b2)
                   (begin (call/cc (lambda (c) (if (not k) (set! k c))))
                          (if again (out 'escaped))
                          (set-car! b2 (car v)))))))
    (parameterize ((p1 'new1) (p2 'new2)) 'body)
    (set-car! b1 'later)
    (let ((r (call/cc (lambda (o) (set! out o) (set! again #t) (k #f)))))
      (list r (car b1) (car b2)))))

;; …and the same when the first entry failed: its setter raised, the handler
;; put the earlier binding back and a `guard` caught the error.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "…however its first entry ended" '(escaped old1 old2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (k #f) (out #f) (again #f)
         (p2 (lambda v
               (if (null? v)
                   (car b2)
                   (begin (call/cc (lambda (c) (if (not k) (set! k c))))
                          (if again (out 'escaped) (raise 'rejected)))))))
    (guard (e (#t #f)) (parameterize ((p1 'new1) (p2 'new2)) 'body))
    (set-car! b1 'later)
    (let ((r (call/cc (lambda (o) (set! out o) (set! again #t) (k #f)))))
      (list r (car b1) (car b2)))))

;; A re-entered setter that returns installs the earlier binding again, so
;; the body runs with both bindings in force each time, and the form's exit
;; restores both, as a body re-entered through `dynamic-wind` would have
;; them.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a re-entered setter that returns installs the earlier binding again"
  '(((new1 new2) (new1 new2)) old1 old2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (k #f) (seen '()) (n 0)
         (p2 (lambda v
               (if (null? v)
                   (car b2)
                   (begin (call/cc (lambda (c) (if (not k) (set! k c))))
                          (set-car! b2 (car v)))))))
    (parameterize ((p1 'new1) (p2 'new2))
      (set! seen (cons (list (car b1) (car b2)) seen)))
    (if (= n 0) (begin (set! n 1) (set-car! b1 'later) (k #f)))
    (list (reverse seen) (car b1) (car b2))))

;; …and one whose first entry failed does too.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "…and so does one whose first entry failed" '(((new1 new2)) old1 old2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (k #f) (again #f) (seen '()) (n 0)
         (p2 (lambda v
               (if (null? v)
                   (car b2)
                   (begin (call/cc (lambda (c) (if (not k) (set! k c))))
                          (if again (set-car! b2 (car v)) (raise 'rejected)))))))
    (guard (e (#t #f))
      (parameterize ((p1 'new1) (p2 'new2))
        (set! seen (cons (list (car b1) (car b2)) seen))))
    (if (= n 0) (begin (set! n 1) (set-car! b1 'later) (set! again #t) (k #f)))
    (list (reverse seen) (car b1) (car b2))))

;; A continuation entering the install from code that left an earlier
;; binding in force: from the form's restore, whose own undo, as the setter
;; restoring `p2` jumps away, puts `d` back to its inner value. The entry
;; counts `d` as put back, which it is not; the escape puts it back to its
;; value before the form all the same, since an exit puts back whatever the
;; install counts.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a jump from the restore into the install puts the earlier binding back"
  '(escaped old1 old2 ((reentered new1 old2)))
  (let* ((d (make-parameter 'old1))
         (b2 (list 'old2)) (k #f) (out #f) (restoring #f) (jumped #f) (log '())
         (p2 (lambda v
               (if (null? v)
                   (car b2)
                   (let ((x (car v)))
                     (if (and restoring (not jumped))
                         (begin (set-car! b2 x) (set! jumped #t) (k #f))
                         (begin
                           (call/cc (lambda (c) (if (not k) (set! k c))))
                           (if jumped
                               (begin (set! log (cons (list 'reentered (d) (car b2)) log))
                                      (out 'escaped)))
                           (set-car! b2 x))))))))
    (let ((r (call/cc
               (lambda (o)
                 (set! out o)
                 (parameterize ((d 'new1) (p2 'new2)) (set! restoring #t) 'body)))))
      (list r (d) (car b2) (reverse log)))))

;; …and from the install's own undo: the third setter escapes, and while the
;; after thunk puts the earlier bindings back, `p2`'s setter jumps into the
;; third install before `p1` is put back. The re-entered install finds them
;; as the interrupted undo left them, as the log shows, and the setter's
;; escape puts both back.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a jump from an install's undo back into it puts the earlier bindings back"
  '(escaped old1 old2 old3 ((jump-from-undo new1 old2) (reentered new1 old2)))
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2)) (b3 (list 'old3))
         (k #f) (out #f) (jumped #f) (log '())
         (p2 (lambda v
               (if (null? v)
                   (car b2)
                   (begin (set-car! b2 (car v))
                          (if (and (eq? (car v) 'old2) (not jumped))
                              (begin (set! jumped #t)
                                     (set! log (cons (list 'jump-from-undo (car b1) (car b2)) log))
                                     (k #f)))))))
         (p3 (lambda v
               (if (null? v)
                   (car b3)
                   (begin
                     (call/cc (lambda (c) (if (not k) (set! k c))))
                     (if jumped (set! log (cons (list 'reentered (car b1) (car b2)) log)))
                     (out (if jumped 'escaped 'first)))))))
    (let ((r (call/cc
               (lambda (o)
                 (set! out o)
                 (parameterize ((p1 'new1) (p2 'new2) (p3 'new3)) 'body)))))
      (list r (car b1) (car b2) (car b3) (reverse log)))))

;; The handler puts the earlier bindings back however the install was
;; entered: here from the restore, which had left `d` in force, the
;; re-entered setter asks, and the handler beyond it sees `d` put back.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "after a jump from the restore the handler sees the earlier binding put back"
  '(answered-out old1 old2 ((handler old1)))
  (let* ((d (make-parameter 'old1))
         (b2 (list 'old2)) (k #f) (out #f) (restoring #f) (jumped #f) (seen '()))
    (define p2
      (lambda v
        (if (null? v)
            (car b2)
            (let ((x (car v)))
              (if (and restoring (not jumped))
                  (begin (set-car! b2 x) (set! jumped #t) (k #f))
                  (begin
                    (call/cc (lambda (c) (if (not k) (set! k c))))
                    (if jumped (raise-continuable 'ask))
                    (set-car! b2 x)))))))
    (with-exception-handler
      (lambda (e) (set! seen (cons (list 'handler (d)) seen)) (out 'answered-out))
      (lambda ()
        (let ((r (call/cc
                   (lambda (o)
                     (set! out o)
                     (parameterize ((d 'new1) (p2 'new2)) (set! restoring #t) 'body)))))
          (list r (d) (car b2) (reverse seen)))))))

;; A setter that raises inside a `parameterize` of its own over an earlier
;; binding: as the error leaves, that inner form restores the earlier binding
;; to its value inside the outer one, after the handler had put it back. The
;; exit puts it back again, so the `guard` finds it as before the form. The
;; same shape with the current output port: a setter that writes its
;; complaint to the error port through `parameterize`.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a setter's own parameterize does not leave an earlier binding in force"
  '((caught rejected) old1 old2)
  (let* ((b1 (list 'old1)) (p1 (make-setter b1))
         (b2 (list 'old2))
         (p2 (lambda v (if (null? v) (car b2) (parameterize ((p1 'x)) (raise 'rejected))))))
    (let ((caught (guard (e (#t (list 'caught e)))
                    (parameterize ((p1 'new1) (p2 'new2)) 'body))))
      (list caught (car b1) (car b2)))))

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

;; ── A continuation captured in a converter (#478) ───────────────────────────
;;
;; Re-entered after the call that ran the converter has returned: the rest of
;; that call runs again with the value delivered, as chibi's and Gauche's
;; converters, which Scheme calls, do. The converter was called from Rust on
;; both backends, which a continuation cannot carry, so the re-entry found
;; nothing to return into — the VM answered `(5 () ())` and the tree-walker
;; abandoned the form. `make-parameter`, `%parameter-convert` and
;; `%parameter-set!` hand the call to the machine now
;; (`patina_primitives::Step`), which runs it as a frame or a continuation.
;;
;; Each captures its continuation on the first conversion of 5 and re-enters
;; it with 10, then 20.
(define (converter-reentered use)
  (let ((k #f) (out '()))
    (let* ((conv (lambda (x) (call/cc (lambda (c) (if (and (not k) (= x 5)) (set! k c)) x))))
           (r (use conv)))
      (set! out (cons r out))
      (if (< (length out) 3) (k (* (length out) 10)) (reverse out)))))

(test-equal "re-entering a converter parameterize ran runs the parameterize again"
  '(5 10 20)
  (converter-reentered
    (lambda (conv) (let ((p (make-parameter 0 conv))) (parameterize ((p 5)) (p))))))

(test-equal "…and one make-parameter ran makes the parameter again" '(5 10 20)
  (converter-reentered
    (lambda (conv) (let ((p (make-parameter 5 conv))) (p)))))

(test-equal "…and one setting the parameter sets it again" '(5 10 20)
  (converter-reentered
    (lambda (conv) (let ((p (make-parameter 0 conv))) (p 5) (p)))))

(test-end)
