;; parameters.scm - Dynamic parameter binding
;;
;; R7RS parameterize macro using dynamic-wind for proper cleanup
;;
;; TODO: For multi-threading support, parameters need thread-local value stacks.
;; Currently parameters use a shared Rc<RefCell<Vec<Value>>> which is fine for
;; single-threaded R7RS-small. For threading, see SRFI-226 (Control Features)
;; which specifies how parameters should interact with threads and continuations.

;; parameterize macro - dynamically bind parameters
;;
;; (parameterize ((param1 val1) (param2 val2) ...) body ...)
;;
;; Evaluates body with parameters bound to new values.
;; On normal or exceptional exit, parameters are restored.
;;
;; Converting and installing are separate steps, in that order, because R7RS
;; §4.2.6 says so and because doing it the other way was two bugs:
;;
;;   - Converting *inside* the before-thunk meant a converter that raised left
;;     the bindings already installed with no after-thunk to undo them. Since
;;     #70 routed program I/O through parameters,
;;     `(parameterize ((current-output-port sink) (current-input-port 5)) …)`
;;     was enough: every later write went to `sink` forever.
;;   - Restoring through the ordinary setter ran the converter a second time,
;;     on a value it had already converted: a doubling converter doubled the
;;     old value on the way out, and a type-changing one made the restore
;;     itself raise.
;;
;; So every new value is converted before the wind is entered, and install and
;; restore both move raw values: through `%parameterize-swap!`, which calls
;; nothing from Rust and installs nothing itself unless every binding is a
;; parameter object. The standard ports are parameter objects since #608,
;; whose converters check the port here, before anything is installed.
;;
;; R7RS makes anything but a parameter object in the binding position an
;; error; Patina accepts a procedure a program wrote to act as one, which reads
;; with no argument and sets with one, and which validates on assignment, with
;; no converter to run early. For a list with one, the swap has the machine
;; call the loop the heap holds, which installs each binding in turn, calling
;; such a procedure as the machine calls any (#784): it used to be called from
;; Rust, which left nothing a continuation could return into (#471) and, on
;; the VM, undid only part of a failed install (#781).
;;
;; `(scheme base)` expands and compiles this file at every start, and a
;; program each `parameterize` it writes, so both are kept small (#817; the
;; expander's cost in depth is #667). The template is the one from before
;; #784: testing the swap's answer in it made each expansion about half as
;; dear again, and passing the swap the loop about 5%. The loop is read from
;; a string and evaluated on first use.

;; What the heap holds as the loop until the first `parameterize` over a
;; procedure a program wrote: it reads the loop from the string below,
;; evaluates it in `(scheme base)`'s environment, hands the heap the loop in
;; its own place, and runs it. A string, so that a start carries one literal
;; and expands none of the loop: as definitions here the loop cost about
;; 2.25 M instructions at every start on the VM, and as a quoted datum about
;; 1.5 M, where the string costs about 0.25 M; its first use costs about 8 M,
;; once. Read from a string, not loaded from a file of its own, so that the
;; first use opens no file, which beneath `environment`'s context could not
;; collect and retry after `EMFILE`, and names no library a program could
;; define in its place.
(define (%parameterize-load-each params vals done)
  (define each
    (eval (read (open-input-string "(let ()
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
  %parameterize-each)"))
          (environment '(scheme base))))
  (%parameterize-set-each! each)
  (each params vals done))

(%parameterize-set-each! %parameterize-load-each)

(define-syntax parameterize
  (syntax-rules ()
    ((parameterize () body ...)
     (begin body ...))
    ((parameterize ((param value) ...) body ...)
     (let* ((params (list param ...))
            (new-vals (map %parameter-convert params (list value ...)))
            (old-vals #f))
       (dynamic-wind
         (lambda ()
           (set! old-vals (%parameterize-swap! params new-vals)))
         (lambda () body ...)
         (lambda ()
           (%parameterize-swap! params old-vals)))))))
