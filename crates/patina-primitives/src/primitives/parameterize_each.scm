;; parameterize_each.scm - `parameterize`'s loop for a binding that is not a
;; parameter object (#784, #817)
;;
;; R7RS makes anything but a parameter object in `parameterize`'s binding
;; position an error; Patina accepts a procedure a program wrote to act as
;; one, which reads with no argument and sets with one. For a list with one,
;; `%parameterize-swap!` installs nothing itself and has the machine call
;; this loop, which installs each binding in turn, calling such a procedure
;; as the machine calls any, and undoes in Scheme (#784, #781).
;;
;; Not a definition in `(scheme base)`, which expands and compiles every
;; form it holds at every start: there it cost about 2.25 M instructions a
;; start on the VM, or as a string about 1.1 M. The swap reads this file's
;; text, which `parameters.rs` includes, on its first use on a heap, has the
;; machine evaluate it in an environment of `(scheme base)`'s exports, which
;; opens no file, and keeps the procedure it answers on the heap (#817). One
;; expression, a `let ()` whose value is the loop.
(let ()
  ;; `%parameterize-each`'s install of one binding: a parameter object raw,
  ;; past its converter, anything else by calling it.
  (define (%parameterize-install! p v)
    (if (not (%parameter-install! p v))
        (p v)))

  ;; Put back the bindings in `done`, records `(p old new)`, each to what
  ;; `pick` takes from its record: `cadr` to undo, `caddr` to install again.
  (define (%parameterize-put! done pick)
    (for-each (lambda (d) (%parameterize-install! (car d) (pick d))) done))

  ;; Install `v` into `p`, with `done` the bindings already made, innermost
  ;; first. A program's setters run inside this bookkeeping, and can raise,
  ;; escape, run a `parameterize` of their own or capture a continuation
  ;; there, so putting `done` back is never skipped:
  ;;   - the handler puts it back before passing a condition on, so an outer
  ;;     handler sees it put back, and a `guard` whose clauses decline,
  ;;     re-raising where the condition was raised (R7RS §4.2.7), changes no
  ;;     binding;
  ;;   - an exit before the setter returns puts it back;
  ;;   - every entry but the first counts it as put back, since whatever
  ;;     left the install since may have put it back, as the form's exit
  ;;     does;
  ;;   - a setter that returns installs it again if it was put back.
  ;; `undone` says whether a returning setter installs `done` again; where it
  ;; errs, the cost is setter calls, an install of what is already in force.
  ;; A re-install that is itself interrupted is #810 and #811. Answers `done`
  ;; with this binding's record added, read before the install.
  (define (%parameterize-install-one! p v done)
    (define old (p))
    (define entered #f)
    (define ok #f)
    (define undone #f)
    (define (put-back!)
      (set! undone #t)
      (%parameterize-put! done cadr))
    (dynamic-wind
      (lambda ()
        (set! ok #f)
        (if entered (set! undone #t) (set! entered #t)))
      (lambda ()
        (with-exception-handler
          (lambda (e) (put-back!) (raise-continuable e))
          (lambda () (%parameterize-install! p v)))
        (if undone
            (begin (set! undone #f) (%parameterize-put! (reverse done) caddr)))
        (set! ok #t))
      (lambda () (if (not ok) (put-back!))))
    (cons (list p old v) done))

  ;; Install `vals` into `params` one by one, left to right, with `done` the
  ;; bindings already made, innermost first, answering the values they held:
  ;; what `%parameterize-swap!` does, for a list with a procedure a program
  ;; wrote. The swap calls it with `done` empty.
  (define (%parameterize-each params vals done)
    (if (null? params)
        (map cadr (reverse done))
        (%parameterize-each
         (cdr params) (cdr vals)
         (%parameterize-install-one! (car params) (car vals) done))))
  %parameterize-each)
