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
;; call a loop, `%parameterize-each`, which installs each binding in turn,
;; calling such a procedure as the machine calls any (#784): it used to be
;; called from Rust, which left nothing a continuation could return into
;; (#471) and, on the VM, undid only part of a failed install (#781).
;;
;; `(scheme base)` expands and compiles this file at every start, and a
;; program each `parameterize` it writes, so both are kept small (#817; the
;; expander's cost in depth is #667). The template is the one from before
;; #784: testing the swap's answer in it made each expansion about half as
;; dear again, and passing the swap the loop about 5%. The loop is not here:
;; the swap compiles it on its first use on a heap
;; (`crates/patina-primitives/src/primitives/parameterize_each.scm`).

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
