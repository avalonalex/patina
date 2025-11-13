;; bootstrap.scm -- Patina internal bootstrap library
;;
;; This file is automatically loaded at interpreter startup.
;; It contains simple derived functions built on top of Rust primitives.
;;
;; Keep this file minimal and dependency-free!
;; Only include functions that:
;;   - Are compositions of existing primitives
;;   - Don't require error handling, I/O, or macros
;;   - Are widely used in R7RS programs

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; TABLE OF CONTENTS
;;
;; - Boolean operations (line 24)
;; - Numeric predicates (line 29)
;; - Car/Cdr compositions (line 47)
;; - Control flow macros (line 84)
;; - Boolean logic macros (line 97)
;; - Binding constructs (line 115)
;; - Multiple value binding (line 147)
;; - Conditional macros (line 175)
;; - Iteration constructs (line 258)

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Boolean operations

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
  (= (remainder x 2) 1))

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
     (let ((x test1))
       (if x x (or test2 ...))))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Binding constructs (R7RS Section 4.2.2)

(define-syntax let
  (syntax-rules ()
    ((let ((name val) ...) body ...)
     ((lambda (name ...) body ...) val ...))))

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

;; NOTE: The standard R7RS definition of 'do' requires nested ellipsis:
;;
;; (define-syntax do
;;   (syntax-rules ()
;;     ((do ((var init step ...) ...)
;;          (test result ...)
;;        command ...)
;;      (let loop ((var init) ...)
;;        (if test
;;            (begin result ...)
;;            (begin
;;              command ...
;;              (loop step ... ...)))))))  ; Nested ellipsis!
;;
;; The nested ellipsis pattern (step ... ...) means "for each binding,
;; expand its step expression, then expand all bindings." This allows
;; optional steps: (var init) or (var init step).
;;
;; Since Patina doesn't yet support nested ellipsis, we use an auxiliary
;; helper macro to normalize bindings first, then expand with single-level
;; ellipsis. This approach has zero runtime overhead compared to the
;; standard definition.

;; R7RS-Compliant do macro with auxiliary helper
;; Handles optional step expressions: (var init) or (var init step)
;; When step is omitted, the variable doesn't change (uses itself as step)
(define-syntax do
  (syntax-rules ()
    ((do (binding ...)
         (test result ...)
       command ...)
     (do-helper (binding ...)
                (test result ...)
                (command ...)
                ()))))  ; empty accumulator for normalized bindings

;; Helper macro to normalize bindings
;; Transforms (var init) -> (var init var) and (var init step) -> (var init step)
(define-syntax do-helper
  (syntax-rules ()
    ;; Base case: all bindings processed, generate letrec
    ((do-helper ()
                (test result ...)
                (command ...)
                ((var init step) ...))
     (letrec ((loop (lambda (var ...)
                      (if test
                          (begin result ...)
                          (begin
                            command ...
                            (loop step ...))))))
       (loop init ...)))

    ;; Recursive case: binding with explicit step
    ((do-helper ((var init step) rest ...)
                test-clause
                commands
                (acc ...))
     (do-helper (rest ...)
                test-clause
                commands
                (acc ... (var init step))))

    ;; Recursive case: binding without step - use var as step
    ((do-helper ((var init) rest ...)
                test-clause
                commands
                (acc ...))
     (do-helper (rest ...)
                test-clause
                commands
                (acc ... (var init var))))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; Testing framework support

;; test macro for (chibi test)
;; Usage: (test expected-value actual-expression)
;;
;; Note: This macro does NOT catch errors during test execution.
;; If expr raises an error, the interpreter's resilient mode will
;; catch it and continue with the next top-level expression.
;; This allows test suites to continue running even when individual
;; tests encounter unimplemented features or runtime errors.

;; Helper for approximate equality (used by test framework)
;; Matches chibi-scheme's behavior: uses epsilon-based comparison for floats
(define (test-equal? expected actual)
  (define epsilon 1e-6)

  (define (approx-equal? x y)
    (cond
      ;; For real numbers, use epsilon-based comparison
      ((and (real? x) (real? y) (not (rational? x)) (not (rational? y)))
       (< (abs (- x y)) epsilon))
      ;; For complex numbers, compare both parts with epsilon
      ((and (complex? x) (complex? y))
       (and (< (abs (- (real-part x) (real-part y))) epsilon)
            (< (abs (- (imag-part x) (imag-part y))) epsilon)))
      ;; For pairs, recurse on both parts
      ((and (pair? x) (pair? y))
       (and (approx-equal? (car x) (car y))
            (approx-equal? (cdr x) (cdr y))))
      ;; For vectors, recurse on all elements
      ((and (vector? x) (vector? y) (= (vector-length x) (vector-length y)))
       (let loop ((i 0))
         (or (>= i (vector-length x))
             (and (approx-equal? (vector-ref x i) (vector-ref y i))
                  (loop (+ i 1))))))
      ;; For everything else, use equal?
      (else (equal? x y))))

  (approx-equal? expected actual))

(define-syntax test
  (syntax-rules ()
    ((test expected expr)
     (let ((expected-val expected))
       ;; First evaluate the expression
       (let ((actual-val expr))
         ;; Then compare and report using approximate equality
         (if (test-equal? expected-val actual-val)
             (begin
               (test-increment-passed)
               #t)  ; Test passed (silently)
             (begin
               (test-increment-failed)
               (display "FAIL: ")
               (newline)
               (write 'expr)
               (newline)
               (display " expected ")
               (newline)
               (write expected-val)
               (newline)
               (display " but got ")
               (newline)
               (write actual-val)
               (newline)
               (newline)
               #f)))))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
;; End of bootstrap library
