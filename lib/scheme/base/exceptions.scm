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

;; The clause walker. Not exported: it is an implementation detail of
;; `guard`, like `%define-field-accessors` is of `define-record-type`.
;; `reraise` is the expression to run when no clause matches.
(define-syntax %guard-aux
  (syntax-rules (else =>)
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
            ;; here — it jumps even when nothing was raised. That jump crosses
            ;; nothing: the body has already returned, so every extent it
            ;; entered is already left, and `guard-k` lands where this return
            ;; lands. The two are equivalent in every observable way.
            ;;
            ;; Returning is what we can afford. On the tree-walker every
            ;; invoke of a reified continuation escapes the nested trampoline
            ;; that Rust primitives call back through, because a
            ;; `CpsContinuation` carries no mark saying which trampoline
            ;; captured it — so the primitive cannot tell a local jump from a
            ;; real escape and runs its cleanup either way. Jumping here would
            ;; make *every* `guard` do that: a `guard` inside a
            ;; `call-with-port` callback closed the port under the still-
            ;; running callback, and Larceny's `base` lost 70 assertions to
            ;; it. Raising still jumps, exactly as it always has, so this
            ;; keeps the exposure where it already was.
            ;;
            ;; Track L §6 has the boundary defect ("an error inside a wind
            ;; thunk escapes `guard`" — same root, the VM's `across_reentry`
            ;; is what it lacks). Restore the reference line when that lands.
            (call-with-values
             (lambda () e1 e2 ...)
             (lambda args
               (lambda ()
                 (apply values args))))))))))))
