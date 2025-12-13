;; parameters.scm - Dynamic parameter binding
;;
;; R7RS parameterize macro using dynamic-wind for proper cleanup
;;
;; TODO: For multi-threading support, parameters need thread-local value stacks.
;; Currently parameters use a shared Rc<RefCell<Vec<Value>>> which is fine for
;; single-threaded R7RS-small. For threading, see SRFI-226 (Control Features)
;; which specifies how parameters should interact with threads and continuations.

;; Helper to set multiple parameters at once
;; Takes a list of (param value) pairs and returns old values
(define (%set-params! params-and-values)
  (map (lambda (pv)
         (let ((param (car pv))
               (value (cadr pv)))
           (let ((old (param)))
             (param value)
             old)))
       params-and-values))

;; Helper to restore parameters from saved values
(define (%restore-params! params old-values)
  (for-each (lambda (p v) (p v)) params old-values))

;; parameterize macro - dynamically bind parameters
;;
;; (parameterize ((param1 val1) (param2 val2) ...) body ...)
;;
;; Evaluates body with parameters bound to new values.
;; On normal or exceptional exit, parameters are restored.
(define-syntax parameterize
  (syntax-rules ()
    ((parameterize () body ...)
     (begin body ...))
    ((parameterize ((param value) ...) body ...)
     (let ((params (list param ...))
           (new-vals (list value ...))
           (old-vals #f))
       (dynamic-wind
         (lambda ()
           (set! old-vals (map (lambda (p) (p)) params))
           (for-each (lambda (p v) (p v)) params new-vals))
         (lambda () body ...)
         (lambda ()
           (for-each (lambda (p v) (p v)) params old-vals)))))))
