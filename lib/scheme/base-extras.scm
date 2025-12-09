;; base-extras.scm -- Derived functions and macros for (scheme base)
;;
;; This file provides R7RS-required procedures and macros that are built
;; on top of the primitive procedures defined in Rust.
;;
;; Part of (scheme base) library.

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Boolean operations (R7RS Section 6.3)

(define (not x)
  (if x #f #t))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Numeric predicates (R7RS Section 6.2.6)

(define (zero? x)
  (= x 0))

(define (positive? x)
  (> x 0))

(define (negative? x)
  (< x 0))

(define (odd? x)
  (not (= (remainder x 2) 0)))

(define (even? x)
  (= (remainder x 2) 0))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Car/Cdr compositions (R7RS Section 6.4)
;; These are required by R7RS and used frequently

(define (caar x) (car (car x)))
(define (cadr x) (car (cdr x)))
(define (cdar x) (cdr (car x)))
(define (cddr x) (cdr (cdr x)))

;; Additional compositions for convenience
(define (caaar x) (car (car (car x))))
(define (caadr x) (car (car (cdr x))))
(define (cadar x) (car (cdr (car x))))
(define (caddr x) (car (cdr (cdr x))))
(define (cdaar x) (cdr (car (car x))))
(define (cdadr x) (cdr (car (cdr x))))
(define (cddar x) (cdr (cdr (car x))))
(define (cdddr x) (cdr (cdr (cdr x))))

;; Four-deep compositions (rarely used, but R7RS includes them)
(define (caaaar x) (car (car (car (car x)))))
(define (caaadr x) (car (car (car (cdr x)))))
(define (caadar x) (car (car (cdr (car x)))))
(define (caaddr x) (car (car (cdr (cdr x)))))
(define (cadaar x) (car (cdr (car (car x)))))
(define (cadadr x) (car (cdr (car (cdr x)))))
(define (caddar x) (car (cdr (cdr (car x)))))
(define (cadddr x) (car (cdr (cdr (cdr x)))))
(define (cdaaar x) (cdr (car (car (car x)))))
(define (cdaadr x) (cdr (car (car (cdr x)))))
(define (cdadar x) (cdr (car (cdr (car x)))))
(define (cdaddr x) (cdr (car (cdr (cdr x)))))
(define (cddaar x) (cdr (cdr (car (car x)))))
(define (cddadr x) (cdr (cdr (car (cdr x)))))
(define (cdddar x) (cdr (cdr (cdr (car x)))))
(define (cddddr x) (cdr (cdr (cdr (cdr x)))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Control flow macros (R7RS Section 4.2.1)

(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

(define-syntax unless
  (syntax-rules ()
    ((unless test body ...)
     (if (not test) (begin body ...)))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Boolean logic macros (R7RS Section 4.2.1)
;; Short-circuiting and/or operators

(define-syntax and
  (syntax-rules ()
    ((and) #t)
    ((and test) test)
    ((and test1 test2 ...)
     (if test1 (and test2 ...) #f))))

(define-syntax or
  (syntax-rules ()
    ((or) #f)
    ((or test) test)
    ((or test1 test2 ...)
     (let ((or-tmp test1))
       (if or-tmp or-tmp (or test2 ...))))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Binding constructs (R7RS Section 4.2.2)

(define-syntax let
  (syntax-rules ()
    ;; Named let (recursive binding) - must come first!
    ;; (let name ((var val) ...) body ...)
    ;; Expands to: ((letrec ((name (lambda (var ...) body ...))) name) val ...)
    ((let proc-name ((var val) ...) body ...)
     ((letrec ((proc-name (lambda (var ...) body ...)))
        proc-name)
      val ...))
    ;; Regular let (parallel bindings)
    ;; (let ((var val) ...) body ...)
    ;; Expands to: ((lambda (var ...) body ...) val ...)
    ((let ((var val) ...) body ...)
     ((lambda (var ...) body ...) val ...))))

(define-syntax let*
  (syntax-rules ()
    ((let* () body ...)
     ((lambda () body ...)))
    ((let* ((name1 val1) (name2 val2) ...) body ...)
     ((lambda (name1)
        (let* ((name2 val2) ...) body ...))
      val1))))

(define-syntax letrec
  (syntax-rules ()
    ((letrec ((var init) ...) body ...)
     (let ((var #f) ...)
       (set! var init) ...
       body ...))))

(define-syntax letrec*
  (syntax-rules ()
    ((letrec* ((var init) ...) body ...)
     (let ((var #f) ...)
       (set! var init) ...
       body ...))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Multiple value binding (R7RS Section 4.2.2)
;;
;; Now using full R7RS-compliant definitions.
;; With call-with-values implemented as a special form (supporting proper
;; tail calls per R7RS Section 3.5), these macro expansions are fully
;; tail-recursive.

;; let-values - bind multiple values from expressions
(define-syntax let-values
  (syntax-rules ()
    ((let-values ((formals expression) rest ...) body ...)
     (call-with-values (lambda () expression)
                       (lambda formals
                         (let-values (rest ...) body ...))))
    ((let-values () body ...)
     (begin body ...))))

;; let*-values - sequential binding of multiple values
;; Each binding can reference values from previous bindings
(define-syntax let*-values
  (syntax-rules ()
    ((let*-values () body0 body1 ...)
     (let () body0 body1 ...))
    ((let*-values (binding0 binding1 ...) body0 body1 ...)
     (let-values (binding0)
       (let*-values (binding1 ...) body0 body1 ...)))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Conditional macros (R7RS Section 4.2.1)

;; cond - multi-branch conditional with else and => support
(define-syntax cond
  (syntax-rules (else =>)
    ;; No clauses - unspecified behavior
    ((cond) (if #f #f))

    ;; Single else clause with => (error case)
    ((cond (else => proc))
     (syntax-error "cond: else clause cannot use =>"))

    ;; Single else clause (base case)
    ((cond (else result1 result2 ...))
     (begin result1 result2 ...))

    ;; Single test clause with =>
    ((cond (test => proc))
     (let ((temp test))
       (if temp (proc temp))))

    ;; Single test clause without expressions (returns test value)
    ((cond (test))
     test)

    ;; Single test clause with expressions
    ((cond (test result1 result2 ...))
     (if test (begin result1 result2 ...)))

    ;; Multiple clauses with => in first
    ((cond (test => proc) clause ...)
     (let ((temp test))
       (if temp
           (proc temp)
           (cond clause ...))))

    ;; Multiple clauses - standard case
    ((cond (test result1 result2 ...) clause ...)
     (if test
         (begin result1 result2 ...)
         (cond clause ...)))))

;; case - pattern matching with eqv? comparison
(define-syntax case
  (syntax-rules (else =>)
    ;; Base case with else and =>
    ((case key (else => proc))
     (proc key))

    ;; Base case with else
    ((case key (else result1 result2 ...))
     (begin result1 result2 ...))

    ;; Base case - no match
    ((case key)
     (if #f #f))

    ;; Single clause with =>
    ((case key ((datum ...) => proc))
     (let ((temp key))
       (if (memv temp '(datum ...))
           (proc temp))))

    ;; Single clause without =>
    ((case key ((datum ...) result1 result2 ...))
     (let ((temp key))
       (if (memv temp '(datum ...))
           (begin result1 result2 ...))))

    ;; Multiple clauses with => in first
    ((case key ((datum ...) => proc) clause ...)
     (let ((temp key))
       (if (memv temp '(datum ...))
           (proc temp)
           (case temp clause ...))))

    ;; Multiple clauses - standard case
    ((case key ((datum ...) result1 result2 ...) clause ...)
     (let ((temp key))
       (if (memv temp '(datum ...))
           (begin result1 result2 ...)
           (case temp clause ...))))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Iteration construct (R7RS Section 4.2.4)
;; Official R7RS definition using auxiliary pattern with literal "step" marker
(define-syntax do
  (syntax-rules ()
    ((do ((var init step ...) ...)
         (test expr ...)
         command ...)
     (letrec
       ((loop
         (lambda (var ...)
           (if test
               (begin
                 (if #f #f)
                 expr ...)
               (begin
                 command
                 ...
                 (loop (do "step" var step ...)
                       ...))))))
       (loop init ...)))
    ((do "step" x)
     x)
    ((do "step" x y)
     y)))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Multiple value binding (R7RS Section 5.3.3)

;; define-values - bind multiple values from an expression
;; Full R7RS-compliant implementation with all pattern types supported
;; Implementation based on chibi-scheme (uses SRFI 46 tail patterns)
(define-syntax define-values
  (syntax-rules ()
    ;; No variables case - evaluate expr for side effects
    ((define-values () expr)
     (define dummy
       (call-with-values (lambda () expr)
         (lambda args #f))))

    ;; Single variable case - just use define
    ((define-values (var) expr)
     (define var expr))

    ;; Two variables case - most common, handle directly
    ((define-values (var0 var1) expr)
     (begin
       (define var0
         (call-with-values (lambda () expr) list))
       (define var1
         (let ((v (cadr var0)))
           (set! var0 (car var0))
           v))))

    ;; Multiple variables case (3+) - uses ellipsis-in-middle pattern
    ((define-values (var0 var1 ... varn) expr)
     (begin
       (define var0
         (call-with-values (lambda () expr) list))
       (define var1
         (let ((v (cadr var0)))
           (set-cdr! var0 (cddr var0))
           v))
       ...
       (define varn
         (let ((v (cadr var0)))
           (set! var0 (car var0))
           v))))

    ;; Dotted list pattern - last variable collects remaining values
    ;; (define-values (x y . z) (values 1 2 3 4)) => x=1, y=2, z=(3 4)
    ((define-values (var0 var1 ... . var-dot) expr)
     (begin
       (define var0
         (call-with-values (lambda () expr) list))
       (define var1
         (let ((v (cadr var0)))
           (set-cdr! var0 (cddr var0))
           v))
       ...
       (define var-dot
         (let ((v (cdr var0)))
           (set! var0 (car var0))
           v))))

    ;; Single variable without list - collects all values as a list
    ;; (define-values x (values 1 2 3)) => x is (1 2 3)
    ((define-values var expr)
     (define var
       (call-with-values (lambda () expr) list)))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Record types (R7RS Section 5.5)
;;
;; define-record-type creates a new record type with:
;; - A type descriptor (bound to <name>)
;; - A constructor procedure
;; - A predicate procedure
;; - Accessor and optional mutator procedures for each field
;;
;; TODO: Add proper error handling once (error) primitive is implemented:
;; - Constructor should check arity
;; - Accessors/mutators should verify record type matches
;; Currently these rely on low-level primitives to error on type mismatches.

;; Helper to create a constructor that maps constructor args to field positions
;; rtd: record type descriptor
;; constructor-fields: list of field names used in constructor (in constructor order)
;; all-fields: list of all field names (in declaration order)
(define (%make-constructor rtd constructor-fields all-fields)
  (let ((num-fields (length all-fields))
        (field-indices
         (map (lambda (cf)
                (%record-type-field-index rtd cf))
              constructor-fields)))
    (lambda args
      (let ((field-vec (make-vector num-fields)))
        ;; Initialize all fields to unspecified
        (do ((i 0 (+ i 1)))
            ((= i num-fields))
          (vector-set! field-vec i (if #f #f)))
        ;; Set the constructor-provided fields
        (do ((args args (cdr args))
             (indices field-indices (cdr indices)))
            ((null? args))
          (vector-set! field-vec (car indices) (car args)))
        (%make-record rtd field-vec)))))

;; Helper macro to define accessor and optional mutator for a field
(define-syntax %define-field-accessors
  (syntax-rules ()
    ;; Field with accessor only
    ((%define-field-accessors rtd field-name accessor)
     (define accessor
       (let ((idx (%record-type-field-index rtd 'field-name)))
         (lambda (record)
           (%record-ref record idx)))))
    ;; Field with accessor and mutator
    ((%define-field-accessors rtd field-name accessor mutator)
     (begin
       (define accessor
         (let ((idx (%record-type-field-index rtd 'field-name)))
           (lambda (record)
             (%record-ref record idx))))
       (define mutator
         (let ((idx (%record-type-field-index rtd 'field-name)))
           (lambda (record value)
             (%record-set! record idx value))))))))

;; Main define-record-type macro
;; Syntax: (define-record-type <name>
;;           (<constructor> <field-name> ...)
;;           <pred>
;;           (<field-name> <accessor>) ...
;;           (<field-name> <accessor> <mutator>) ...)
(define-syntax define-record-type
  (syntax-rules ()
    ((define-record-type name
       (constructor constructor-field ...)
       pred
       (field-name accessor . maybe-mutator) ...)
     (begin
       ;; Create the record type descriptor
       (define name (%make-record-type 'name '(field-name ...)))

       ;; Create the predicate
       (define (pred obj)
         (and (%record? obj)
              (eq? (%record-type-of obj) name)))

       ;; Create the constructor
       (define constructor
         (%make-constructor name
                            '(constructor-field ...)
                            '(field-name ...)))

       ;; Create accessors and mutators for each field
       (%define-field-accessors name field-name accessor . maybe-mutator)
       ...))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; End of (scheme base) extras
