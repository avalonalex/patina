;; Exception handling macros for (scheme base)
;;
;; R7RS Section 4.2.7 - Exception handling

;; (guard (var clause ... [clause]) body1 body2 ...)
;;
;; The guard form establishes a handler that catches exceptions raised
;; during evaluation of the body expressions. If an exception is raised,
;; the clauses are evaluated like cond clauses, with var bound to the
;; raised object. If no clause matches and there's no else clause,
;; the exception is re-raised.
;;
;; This is R7RS 7.3's reference expansion, bar one deliberate deviation on the
;; success path that is explained where it happens. The two continuations are
;; the whole point of the form:
;;
;;   `guard-k`   jumps *out* to the guard's own dynamic environment. R7RS
;;               4.2.7 evaluates the clauses there, not at the raise point,
;;               and this jump is what runs the after-thunks of any extent
;;               the body had entered.
;;
;;   `handler-k` jumps *back in* to the raise point, so a clause that
;;               declines re-raises inside the extent it came from. The
;;               before-thunks run again on the way in.
;;
;; An earlier expansion ran the clauses inside the handler and called
;; `guard-k` with their result. That reads as simpler, but it puts the
;; clauses in the wrong dynamic environment and leaves no way back to the
;; raise point, so a declining clause could not re-raise where R7RS says it
;; must (Track L triage family 22).

;; The clause walker. `reraise` is the expression to run when no clause
;; matches.
;;
;; Not exported, which is the *minority* choice here: `%define-field-accessors`,
;; `%make-constructor` and `%parameterize-swap!` are all exported from
;; `(scheme base)` despite being macro internals. Keeping this one out is
;; deliberate and verified — `guard`'s template resolves it hygienically at the
;; definition site, checked through `(only (scheme base) guard raise)` and
;; `(prefix (scheme base) s:)`. Export it only if a resolution failure forces
;; the issue.
(define-syntax %guard-aux
  (syntax-rules (else =>)
    ;; No clauses at all. R7RS 7.1.3 allows it (`(guard (<identifier>
    ;; <cond clause>*) <body>)`), and 7.3's reference `guard-aux` does not
    ;; cover it — a `(guard (e) body)` would fail there with a message naming
    ;; this helper, an identifier the user never wrote. Re-raise, which is
    ;; what "no clause matched" means, on both raise forms.
    ((%guard-aux reraise)
     reraise)

    ((%guard-aux reraise (else result1 result2 ...))
     (begin result1 result2 ...))

    ((%guard-aux reraise (test => result))
     (let ((temp test))
       (if temp (result temp) reraise)))

    ((%guard-aux reraise (test => result) clause1 clause2 ...)
     (let ((temp test))
       (if temp
           (result temp)
           (%guard-aux reraise clause1 clause2 ...))))

    ((%guard-aux reraise (test))
     (or test reraise))

    ((%guard-aux reraise (test) clause1 clause2 ...)
     (let ((temp test))
       (if temp
           temp
           (%guard-aux reraise clause1 clause2 ...))))

    ((%guard-aux reraise (test result1 result2 ...))
     (if test (begin result1 result2 ...) reraise))

    ((%guard-aux reraise (test result1 result2 ...) clause1 clause2 ...)
     (if test
         (begin result1 result2 ...)
         (%guard-aux reraise clause1 clause2 ...)))))

(define-syntax guard
  (syntax-rules ()
    ((guard (var clause ...) e1 e2 ...)
     ;; Both arms of `with-exception-handler` produce a *thunk*, which the
     ;; outer application runs once control is back here. That is what puts
     ;; the clauses, and the body's own result, in the guard's dynamic
     ;; environment rather than the raise's.
     ((call-with-current-continuation
       (lambda (guard-k)
         (with-exception-handler
          (lambda (condition)
            ((call-with-current-continuation
              (lambda (handler-k)
                (guard-k
                 (lambda ()
                   (let ((var condition))
                     (%guard-aux
                      (handler-k
                       (lambda ()
                         (raise-continuable condition)))
                      clause ...))))))))
          (lambda ()
            ;; The body may return any number of values, so they travel out
            ;; packed in a list and are unpacked by the outer application.
            ;;
            ;; **One deviation from R7RS 7.3, and it is deliberate.** The
            ;; reference writes `(guard-k (lambda () (apply values args)))`
            ;; here — it jumps even when nothing was raised. That jump is
            ;; equivalent to returning only when no composable continuation
            ;; is involved: `guard-k` is a *full* continuation captured when
            ;; the `guard` was entered, so if the body is resumed through a
            ;; composable continuation from somewhere else, the jump goes
            ;; back to the original context instead of returning to the
            ;; invoker. Racket 9.3 and Guile 3.0.11 return; so does this.
            ;; On the VM the jump would also cost a second O(depth) frame
            ;; copy per `guard`. `tests/scheme/control/prompts.scm` pins it.
            ;;
            ;; It was first a workaround — the tree-walker's nested
            ;; trampoline could not tell a local jump from an escape, and a
            ;; `guard` inside a `call-with-port` callback closed the port
            ;; under the callback — and that reason went on 2026-09-10.
            ;; Restoring the line was then measured and rejected for the
            ;; reason above (Track L §6, under "a primitive's callback runs
            ;; on a nested trampoline").
            ;;
            ;; The *raise* path has the same flaw and cannot avoid it here:
            ;; a clause's result goes out through `guard-k` too, so a
            ;; composable continuation resumed through a `guard` whose body
            ;; raises still jumps to the original context. The same file pins
            ;; that as an expected failure on both backends; the fix is a
            ;; `guard` built on a prompt of its own, as Racket's and Guile's
            ;; are.
            (call-with-values
             (lambda () e1 e2 ...)
             (lambda args
               (lambda ()
                 (apply values args))))))))))))
