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

;; The carry operators always split at this one modulus, so it and its
;; half-point are computed once at library load — both are bignums, and
;; rebuilding them per call measured ~10-15% of each carry operation.
(define fx-modulus (expt 2 fx-width))
(define fx-half (quotient fx-modulus 2))

;; Balanced division by fx-modulus, as in SRFI 141: the remainder lands in
;; [-fx-half, fx-half), which is what the carry operators need to split a wide
;; result into a fixnum result plus a carry. Defined here rather than importing
;; SRFI 141 for one specialized procedure.
;;
;; Computed via floor-based modulo, NOT (round (/ n d)): round's half-to-even
;; tie break puts an exactly-half remainder on the excluded endpoint +fx-half,
;; so (fx+/carry fx-greatest 1 0) would return the non-fixnum 2^60 instead of
;; (fx-least 1) — the exact boundary the carry operators exist to handle.
(define (balanced/ n)
  (let* ((r0 (modulo n fx-modulus))
         (r (if (>= r0 fx-half) (- r0 fx-modulus) r0)))
    (values (quotient (- n r) fx-modulus) r)))

(define (fx+/carry i j k)
  (call-with-values (lambda () (balanced/ (+ i j k)))
    (lambda (q r) (values r q))))

(define (fx-/carry i j k)
  (call-with-values (lambda () (balanced/ (- i j k)))
    (lambda (q r) (values r q))))

(define (fx*/carry i j k)
  (call-with-values (lambda () (balanced/ (+ (* i j) k)))
    (lambda (q r) (values r q))))

(define fxarithmetic-shift-left fxarithmetic-shift)
(define (fxarithmetic-shift-right i count)
  (fxarithmetic-shift i (- count)))
