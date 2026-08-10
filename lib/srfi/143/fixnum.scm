;;; SRFI 143 fixnum operations.
;;;
;;; Most are the ordinary numeric procedures under fx names: on a fixnum
;;; argument they already do the right thing, and SRFI 143 leaves behaviour on
;;; a non-fixnum argument unspecified.

(define fx=? =)
(define fx<? <)
(define fx>? >)
(define fx<=? <=)
(define fx>=? >=)
(define fxzero? zero?)
(define fxpositive? positive?)
(define fxnegative? negative?)
(define fxodd? odd?)
(define fxeven? even?)
(define fxmax max)
(define fxmin min)
(define fx+ +)
(define fx- -)
(define fx* *)
(define fxquotient quotient)
(define fxremainder remainder)
(define fxabs abs)
(define fxsquare square)
(define fxsqrt exact-integer-sqrt)
(define (fxneg x) (- x))

;; Derived from the representation rather than hardcoded, so it cannot claim a
;; range the tagging does not provide. Patina's fixnums are 61-bit signed today;
;; probing keeps this correct if that ever changes.
(define fx-width
  (let loop ((n 1) (w 1))
    (if (fixnum? n)
        (loop (* n 2) (+ w 1))
        w)))

(define fx-greatest (- (expt 2 (- fx-width 1)) 1))
(define fx-least (- (expt 2 (- fx-width 1))))

;; Balanced division, from SRFI 141: the remainder lands in [-d/2, d/2), which
;; is what the carry operators need to split a wide result into a fixnum result
;; plus a carry. Defined here rather than importing SRFI 141 for one procedure.
(define (balanced/ n d)
  (let* ((q (round (/ n d)))
         (r (- n (* q d))))
    (values (exact q) (exact r))))

(define (fx+/carry i j k)
  (call-with-values (lambda () (balanced/ (+ i j k) (expt 2 fx-width)))
    (lambda (q r) (values r q))))

(define (fx-/carry i j k)
  (call-with-values (lambda () (balanced/ (- i j k) (expt 2 fx-width)))
    (lambda (q r) (values r q))))

(define (fx*/carry i j k)
  (call-with-values (lambda () (balanced/ (+ (* i j) k) (expt 2 fx-width)))
    (lambda (q r) (values r q))))

(define fxarithmetic-shift-left fxarithmetic-shift)
(define (fxarithmetic-shift-right i count)
  (fxarithmetic-shift i (- count)))
