;; binding.scm -- Binding constructs for (scheme base)
;;
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

;; Multiple value binding (R7RS Section 4.2.2)

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

;; define-values - bind multiple values from an expression
;; Full R7RS-compliant implementation with all pattern types supported
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
