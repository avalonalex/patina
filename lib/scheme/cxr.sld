;; (scheme cxr) - R7RS Extended Car/Cdr Library
;;
;; Four-deep car/cdr compositions.
;; Three-deep and below are in (scheme base).

(define-library (scheme cxr)
  (import (patina internal lists))

  (export
    ;; All compositions up to four deep are defined here
    ;; (scheme base) provides up to three deep)
    caaaar caaadr caadar caaddr
    cadaar cadadr caddar cadddr
    cdaaar cdaadr cdadar cdaddr
    cddaar cddadr cdddar cddddr)

  (begin
    ;; Four-deep compositions
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
    (define (cddddr x) (cdr (cdr (cdr (cdr x)))))))
