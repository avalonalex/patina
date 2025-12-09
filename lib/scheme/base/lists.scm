;; lists.scm -- List accessors and predicates for (scheme base)
;;
;; Car/Cdr compositions (R7RS Section 6.4)
;; These are required by R7RS and used frequently

(define (not x)
  (if x #f #t))

;; Two-deep compositions
(define (caar x) (car (car x)))
(define (cadr x) (car (cdr x)))
(define (cdar x) (cdr (car x)))
(define (cddr x) (cdr (cdr x)))

;; Three-deep compositions
(define (caaar x) (car (car (car x))))
(define (caadr x) (car (car (cdr x))))
(define (cadar x) (car (cdr (car x))))
(define (caddr x) (car (cdr (cdr x))))
(define (cdaar x) (cdr (car (car x))))
(define (cdadr x) (cdr (car (cdr x))))
(define (cddar x) (cdr (cdr (car x))))
(define (cdddr x) (cdr (cdr (cdr x))))

;; Four-deep compositions (rarely used, but R7RS includes them in (scheme cxr))
;; For (scheme base), we only provide up to three-deep per R7RS
;; The four-deep ones will be in (scheme cxr)
