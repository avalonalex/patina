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
;; End of bootstrap library
