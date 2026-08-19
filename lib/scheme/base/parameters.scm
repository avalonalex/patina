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
;; restore both move raw values through `%parameterize-swap!` — which is also
;; transactional, because not everything `parameterize` accepts is a parameter
;; *object*: the standard ports are procedures over a thread-local, so they
;; have no converter to run early and validate on assignment instead. See its
;; doc comment in `patina-primitives`.
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
