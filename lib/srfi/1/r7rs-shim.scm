;;; R7RS compatibility shim for SRFI 1 reference implementation
;;;
;;; Provides the non-R7RS constructs used by srfi-1-reference.scm:
;;; - check-arg: parameter validation
;;; - receive: SRFI 8 multiple-value binding
;;; - let-optionals / :optional: optional argument parsing

;; check-arg: validate a parameter, loop on error
(define (check-arg pred val caller)
  (if (pred val)
      val
      (error "Bad argument" val caller)))

;; receive (SRFI 8): bind multiple values
(define-syntax receive
  (syntax-rules ()
    ((receive formals expr body ...)
     (call-with-values (lambda () expr)
       (lambda formals body ...)))))

;; :optional — extract an optional argument from a rest list
(define-syntax :optional
  (syntax-rules ()
    ((:optional rest default-exp)
     (if (null? rest) default-exp (car rest)))
    ((:optional rest default-exp pred)
     (let ((val (if (null? rest) default-exp (car rest))))
       (if (pred val) val
           (error "Bad optional argument" val))))))

;; let-optionals — bind optional arguments from a rest list
(define-syntax let-optionals
  (syntax-rules ()
    ((let-optionals rest () body ...)
     (begin body ...))
    ((let-optionals rest ((var default) . more) body ...)
     (let ((var (if (null? rest) default (car rest)))
           (rest* (if (null? rest) '() (cdr rest))))
       (let-optionals rest* more body ...)))
    ((let-optionals rest (var . more) body ...)
     (let ((var (car rest))
           (rest* (cdr rest)))
       (let-optionals rest* more body ...)))))
