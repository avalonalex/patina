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
;; nothing and installs nothing unless every binding is a parameter object.
;; The standard ports are parameter objects since #608, whose converters check
;; the port here, before anything is installed.
;;
;; R7RS makes anything but a parameter object in the binding position an
;; error; Patina accepts a procedure a program wrote to act as one, which reads
;; with no argument and sets with one, and which validates on assignment, with
;; no converter to run early. For a list with one, the swap answers #f and
;; `%parameterize-each` installs each binding in turn, calling such a
;; procedure as the machine calls any (#784): it used to be called from Rust,
;; which left nothing a continuation could return into (#471) and, on the
;; VM, undid only part of a failed install (#781).

;; `%parameterize-each`'s install of one binding: a parameter object raw,
;; past its converter, anything else by calling it.
(define (%parameterize-install! p v)
  (if (not (%parameter-install! p v))
      (p v)))

;; Install `vals` into `params` one by one, left to right, answering the
;; values they held: what `%parameterize-swap!` does, for a list with a
;; procedure a program wrote. An install that does not return puts back the
;; bindings already made, innermost first. On an error, the handler puts them
;; back before passing the condition on, so an outer handler, and a program
;; that leaves the error uncaught, sees them put back; if that handler answers
;; and the setter returns, they are installed again. On an escape the after
;; thunk puts them back. `ok` is set false each time the install is entered,
;; so a re-entered setter that escapes undoes too.
(define (%parameterize-each params vals)
  (let loop ((ps params) (vs vals) (done '()))
    (if (null? ps)
        (map cadr (reverse done))
        (let* ((p (car ps)) (v (car vs)) (old (p)) (ok #f) (undone #f))
          (define (undo!)
            (unless undone
              (set! undone #t)
              (for-each (lambda (d) (%parameterize-install! (car d) (cadr d))) done)))
          (dynamic-wind
            (lambda () (set! ok #f))
            (lambda ()
              (with-exception-handler
                (lambda (e) (undo!) (raise-continuable e))
                (lambda () (%parameterize-install! p v)))
              (when undone
                (set! undone #f)
                (for-each (lambda (d) (%parameterize-install! (car d) (caddr d)))
                          (reverse done)))
              (set! ok #t))
            (lambda () (unless ok (undo!))))
          (loop (cdr ps) (cdr vs) (cons (list p old v) done))))))

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
           (set! old-vals (%parameterize-swap! params new-vals))
           (if (not old-vals)
               (set! old-vals (%parameterize-each params new-vals))))
         (lambda () body ...)
         (lambda ()
           (if (not (%parameterize-swap! params old-vals))
               (%parameterize-each params old-vals))))))))
