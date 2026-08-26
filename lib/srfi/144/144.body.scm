;;; Copyright (C) William D Clinger (2016).
;;;
;;; Permission is hereby granted, free of charge, to any person
;;; obtaining a copy of this software and associated documentation
;;; files (the "Software"), to deal in the Software without
;;; restriction, including without limitation the rights to use,
;;; copy, modify, merge, publish, distribute, sublicense, and/or
;;; sell copies of the Software, and to permit persons to whom the
;;; Software is furnished to do so, subject to the following
;;; conditions:
;;;
;;; The above copyright notice and this permission notice shall be
;;; included in all copies or substantial portions of the Software.
;;;
;;; THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
;;; EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES
;;; OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
;;; NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
;;; HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
;;; WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
;;; FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
;;; OTHER DEALINGS IN THE SOFTWARE.

;;; References
;;;
;;; Milton Abramowitz and Irene A Stegun [editors].
;;; Handbook of Mathematical Functions With Formulas, Graphs, and
;;; Mathematical Tables.  United States Department of Commerce.
;;; National Bureau of Standards Applied Mathematics Series, 55,
;;; June 1964.  Fifth Printing, August 1966, with corrections.
;;;
;;; R W Hamming.  Numerical Methods for Scientists and Engineers.
;;; McGraw-Hill, 1962.
;;;
;;; Donald E Knuth.  The Art of Computer Programming, Volume 2,
;;; Seminumerical Algorithms, Second Edition.  Addison-Wesley, 1981.
;;;
;;; J N Newman.  Approximations for the Bessel and Struve Functions.
;;; Mathematics of Computation, 43(168), October 1984, pages 551-556.

;;; I have deliberately avoided recent references, and have also
;;; avoided looking at any code or pseudocode for these or similar
;;; functions.

;;; Quick-and-dirty implementation of a draft of SRFI 144 (flonums),
;;; as specified at http://vrici.lojban.org/~cowan/temp/srfi-144.html
;;; as of 4 June 2017.
;;;
;;; FIXME: not as accurate as it should be
;;; FIXME: not as fast as it should be
;;; FIXME: assumes IEEE arithmetic or similar
;;; FIXME: assumes all inexact reals are flonums
;;; FIXME: assumes (scheme inexact)

;;; Mathematical Constants
;;;
;;; The mathematical constants are now defined in 144.constants.scm


;; Implementation Constants

(define fl-greatest
  (let loop ((x (- (expt 2.0 precision-bits) 1.0)))
    (if (finite? (* 2.0 x))
        (loop (* 2.0 x))
        x)))

(define fl-least
  (let loop ((x 1.0))
    (if (> (/ x 2.0) 0.0)
        (loop (/ x 2.0))
        x)))

(define fl-epsilon
  (let loop ((eps 1.0))
    (if (= 1.0 (+ 1.0 eps))
        (* 2.0 eps)
        (loop (/ eps 2.0)))))

(define fl-integer-exponent-zero                ; arbitrary
  (exact (- (log fl-least 2.0) 1.0)))

(define fl-integer-exponent-nan                 ; arbitrary
  (- fl-integer-exponent-zero 1))

;;; Constructors

; Implements post-finalization note 1
(define (flonum x)
  (if (real? x)
      (inexact x)
      +nan.0))

(define fladjacent
  (flop2 'fladjacent
         (lambda (x y)
           (define (loop y)
             (let* ((y3 (fl+ (fl* 0.999755859375 x) (fl* 0.000244140625 y))))
               (cond ((fl<? x y3 y)
                      (loop y3))
                     ((fl<? y y3 x)
                      (loop y3))
                     (else
                      (loop2 y)))))
           (define (loop2 y)
             (let* ((y2 (fl/ (fl+ x y) 2.0))
                    (y2 (if (flinfinite? y2)
                            (fl+ (fl* 0.5 x) (fl* 0.5 y))
                            y2)))
               (cond ((fl=? x y2)
                      y)
                     ((fl=? y y2)
                      y)
                     (else
                      (loop2 y2)))))
           (cond ((flinfinite? x)
                  (cond ((fl<? x y) (fl- fl-greatest))
                        ((fl>? x y) fl-greatest)
                        (else x)))
                 ((fl=? x y)
                  x)
                 ((flzero? x)
                  (if (flpositive? y)
                      fl-least
                      (fl- fl-least)))
                 ((fl<? x y)
                  (loop (flmin y
                               fl-greatest
                               (flmax (* 2.0 x)
                                      (* 0.5 x)))))
                 ((fl>? x y)
                  (loop (flmax y
                               (fl- fl-greatest)
                               (flmin (* 2.0 x)
                                      (* 0.5 x)))))
                 (else    ; x or y is a NaN
                  x)))))

(define flcopysign
  (flop2 'flcopysign
         (lambda (x y)
           (if (= (flsign-bit x) (flsign-bit y))
               x
               (fl- x)))))

(define (make-flonum x n)
  (let ((y (expt 2.0 n)))
    (cond ((or (not (flonum? x))
               (not (exact-integer? n)))
           (error "bad arguments to make-flonum" x n))
          ((finite? y)
           (* x y))
          (else
           (inexact (* (exact x) (expt 2 n)))))))

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;

;;; Accessors

(define (flinteger-fraction x)
  (check-flonum! 'flinteger-fraction x)
  (let* ((result1 (fltruncate x))
         (result2 (fl- x result1)))
    (values result1 result2)))

(define (flexponent x)
  (floor (fllog2 (flabs x))))

(define (flinteger-exponent x)
  (exact (flexponent x)))

(define (flnormalized-fraction-exponent x)
  (define (return result1 result2)
    (cond ((fl<? result1 0.5)
           (values (fl* 2.0 result1) (- result2 1)))
          ((fl>=? result1 1.0)
           (values (fl* 0.5 result1) (+ result2 1)))
          (else
           (values result1 result2))))
  (check-flonum! 'flnormalized-fraction-exponent x)
  (cond ((flnan? x)    ; unspecified for NaN
         (values x 0))
        ((fl<? x 0.0)
         (call-with-values
          (lambda () (flnormalized-fraction-exponent (fl- x)))
          (lambda (y n)
            (values (fl- y) n))))
        ((fl=? x 0.0)    ; unspecified for 0.0
         (values 0.0 0))
        ((flinfinite? x)
         (values 0.5 (+ 3 (exact (round (fllog2 fl-greatest))))))
        ((flnormalized? x)
         (let* ((result2 (exact (flround (fllog2 x))))
                (result2 (if (integer? result2)
                             result2
                             (round result2)))
                (two^result2 (inexact (expt 2.0 result2))))
           (if (flinfinite? two^result2)
               (call-with-values
                (lambda () (flnormalized-fraction-exponent (fl/ x 4.0)))
                (lambda (y n)
                  (values y (+ n 2))))
               (return (fl/ x two^result2) result2))))
        (else
         (let* ((k (+ 2 precision-bits))
                (two^k (expt 2 k)))
           (call-with-values
            (lambda ()
              (flnormalized-fraction-exponent (fl* x (inexact two^k))))
            (lambda (y n)
              (return y (- n k))))))))

(define (flsign-bit x)
  (check-flonum! 'flsign-bit x)
  (cond ((fl<? x 0.0)
         1)
        ((eqv? x -0.0)
         1)
        (else
         0)))


;;; Predicates

;(define flonum? R6RS)               ; defined by (rnrs arithmetic flonums)
;(define fl=? R6RS)                  ; defined by (rnrs arithmetic flonums)
;(define fl<? R6RS)                  ; defined by (rnrs arithmetic flonums)
;(define fl>? R6RS)                  ; defined by (rnrs arithmetic flonums)
;(define fl<=? R6RS)                 ; defined by (rnrs arithmetic flonums)
;(define fl>=? R6RS)                 ; defined by (rnrs arithmetic flonums)

(define (flunordered? x y)
  (or (flnan? x) (flnan? y)))

;;; incompatible with (rnrs arithmetic flonums) in zero-argument case

(define flmax
  (let ((flmax2 (flop2 'flmax max)))
    (lambda args
      (cond ((null? args)
             -inf.0)
            ((null? (cdr args))
             (car args))
            ((null? (cddr args))
             (flmax2 (car args) (cadr args)))
            (else
             (flmax2 (flmax2 (car args) (cadr args))
                     (apply flmax (cddr args))))))))

;;; incompatible with (rnrs arithmetic flonums) in zero-argument case

(define flmin
  (let ((flmin2 (flop2 'flmin min)))
    (lambda args
      (cond ((null? args)
             +inf.0)                 ; spec says fl-least, but that's wrong
            ((null? (cdr args))
             (car args))
            ((null? (cddr args))
             (flmin2 (car args) (cadr args)))
            (else
             (flmin2 (flmin2 (car args) (cadr args))
                     (apply flmin (cddr args))))))))

;(define flinteger? R6RS)            ; defined by (rnrs arithmetic flonums)
;(define flzero? R6RS)               ; defined by (rnrs arithmetic flonums)
;(define flpositive? R6RS)           ; defined by (rnrs arithmetic flonums)
;(define flnegative? R6RS)           ; defined by (rnrs arithmetic flonums)
;(define flodd? R6RS)                ; defined by (rnrs arithmetic flonums)
;(define fleven? R6RS)               ; defined by (rnrs arithmetic flonums)
;(define flfinite? R6RS)             ; defined by (rnrs arithmetic flonums)
;(define flinfinite? R6RS)           ; defined by (rnrs arithmetic flonums)
;(define flnan? R6RS)                ; defined by (rnrs arithmetic flonums)

(define flnormalized?
  (lambda (x)
    (check-flonum! 'flnormalized? x)
    (let ((x (flabs x)))
      (and (flfinite? x)
           (fl<? (fl/ fl-greatest) x)))))

(define fldenormalized?
  (lambda (x)
    (check-flonum! 'fldenormalized? x)
    (let ((x (flabs x)))
      (and (flfinite? x)
           (fl<? 0.0 x)
           (fl<=? x (fl/ fl-greatest))))))

;;; Arithmetic

;(define fl+ R6RS)                   ; defined by (rnrs arithmetic flonums)
;(define fl* R6RS)                   ; defined by (rnrs arithmetic flonums)

;;; Spec says "as if to infinite precision and rounded only once".

(define fl+*
  (flop3 'fl+*
         (lambda (x y z)
           (cond (c-functions-are-available
                  (fma x y z))
                 ((or (not (flfinite? x))
                      (not (flfinite? y))
                      (flzero? x)
                      (flzero? y))
                  ;; If one of x or y is not finite, then the answer is
                  ;; infinite or NaN, as determined by the floating-point
                  ;; operations.
                  ;; If x or y is a signed zero, and the other is finite,
                  ;; then the product will be a zero with the correct sign,
                  ;; and the sum will be correct, even if z is a signed zero.
                  (fl+ (fl* x y) z))
                 ((flfinite? z)
                  ;; everything is finite and x and y are nonzero.
                  (let ((x (exact x))
                        (y (exact y))
                        (z (exact z)))
                    ;; if (+ (* x y) z) is zero, i.e., z = -(x*y), then
                    ;; the result will be +0., which is correct.
                    (inexact (+ (* x y) z))))
                 (else
                  ;; x and y are finite, so adding (* x y) to the
                  ;; non-finite z is just z.
                  z)))))

;(define fl- R6RS)                   ; defined by (rnrs arithmetic flonums)
;(define fl/ R6RS)                   ; defined by (rnrs arithmetic flonums)
;(define flabs R6RS)                 ; defined by (rnrs arithmetic flonums)

(define (flabsdiff x y)
  (flabs (fl- x y)))

(define (flposdiff x y)
  (let ((diff (fl- x y)))
    (if (flnegative? diff)
        0.0
        diff)))

(define (flsgn x)
  (flcopysign 1.0 x))

;;; (flnumerator +nan.0) and (fldenominator +nan.0) must be NaNs, which
;;; is not required by the R6RS specification of (rnrs arithmetic flonums).

(define flnumerator
  (flop1 'flnumerator
         (lambda (x)
           (if (flnan? x)
               x
               (r6rs:flnumerator x)))))

(define fldenominator
  (flop1 'fldenominator
         (lambda (x)
           (if (flnan? x)
               x
               (r6rs:fldenominator x)))))

;(define flfloor R6RS)               ; defined by (rnrs arithmetic flonums)
;(define flceiling R6RS)             ; defined by (rnrs arithmetic flonums)
;(define flround R6RS)               ; defined by (rnrs arithmetic flonums)
;(define fltruncate R6RS)            ; defined by (rnrs arithmetic flonums)

;;; Exponents and logarithms

;(define flexp R6RS)                 ; defined by (rnrs arithmetic flonums)

(define flexp2 (flop1 'flexp2 (lambda (x) (flexpt 2.0 x))))

;;; e^x = \sum_n (z^n / (n!))
;;;
;;; FIXME: the number of terms and the constant 0.5 seem reasonable
;;; for IEEE double precision, but the number of terms might need
;;; to be increased for higher precisions.

(define flexp-1
  (flop1 'flexp-1
         (let ((coefs (cons 0.0
                            (map fl/
                                 (map factorial
                                      '(1.0 2.0 3.0 4.0 5.0
                                        6.0 7.0 8.0 9.0 10.0
                                        11.0 12.0 13.0 14.0 15.0))))))
           (lambda (x)
             (cond ((fl<? (flabs x) 0.5)    ; FIXME
                    (polynomial-at x coefs))
                   (else
                    (fl- (flexp x) 1.0)))))))

(define flsquare (flop1 'flsquare (lambda (x) (fl* x x))))

;(define flsqrt R6RS)                ; defined by (rnrs arithmetic flonums)

(define flcbrt
  (flop1 'flcbrt
         (lambda (x)
           (cond ((flnegative? x)
                  (fl- (flcbrt (fl- x))))
                 (else
                  (flexpt x (fl/ 3.0)))))))

(define flhypot
  (flop2 'flhypot
         (lambda (x y)
           (cond ((flzero? x) (flabs y))
                 ((flzero? y) (flabs x))
                 ((or (flinfinite? x) (flinfinite? y)) +inf.0)
                 ((flnan? x) x)
                 ((flnan? y) y)
                 ((fl>? y x) (flhypot y x))
                 (else
                  (let* ((y/x (fl/ y x))
                         (root (flsqrt (fl+ 1.0 (fl* y/x y/x)))))
                    (fl* (flabs x) root)))))))

;(define flexpt R6RS)                ; defined by (rnrs arithmetic flonums)
;(define fllog R6RS)                 ; defined by (rnrs arithmetic flonums)

;;; Returns log(x+1), as in C99 log1p.

;;; See

;;; https://stat.ethz.ch/pipermail/r-devel/2003-August/027396.html
;;; https://books.google.com/books?id=OjUyDwAAQBAJ&pg=PA290&lpg=PA290&dq=beebe+log1p&source=bl&ots=VLxmiSk1fA&sig=ACfU3U0_8tqKemomSjKW73iJ0zUO1u3p3Q&hl=en&sa=X&ved=2ahUKEwjfxZbE8LvhAhVNm-AKHWScB7w4ChDoATAAegQICRAB#v=onepage&q=beebe%20log1p&f=false

;;; for justification


(define fllog1+
  (flop1 'fllog1+
         (lambda (x)
           (let ((u (fl+ 1.0 x)))
             (cond ((fl=? u 1.0)
                    x) ;; gets sign of zero result correct
                   ((fl=? u x)
                    (fllog u)) ;; large arguments and infinities
                   (else
                    (fl* (fllog u) (fl/ x (fl- u 1.0)))))))))


(define fllog2 (flop1 'fllog2 (lambda (x) (log x 2.0))))

(define fllog10 (flop1 'fllog10 (lambda (x) (log x 10.0))))

(define (make-fllog-base base)
  (check-flonum! 'make-fllog-base base)
  (if (fl>? base 1.0)
      (flop1 'procedure-created-by-make-fllog-base
             (lambda (x) (log x base)))
      (error "argument to make-fllog-base must be greater than 1.0" base)))

;;; Trigonometric functions

;(define flsin R6RS)                 ; defined by (rnrs arithmetic flonums)
;(define flcos R6RS)                 ; defined by (rnrs arithmetic flonums)
;(define fltan R6RS)                 ; defined by (rnrs arithmetic flonums)
;(define flasin R6RS)                ; defined by (rnrs arithmetic flonums)
;(define flacos R6RS)                ; defined by (rnrs arithmetic flonums)
;(define flatan R6RS)                ; defined by (rnrs arithmetic flonums)

(define flsinh
  (flop1 'flsinh
         (lambda (x)
           (cond ((not (flfinite? x)) x)
                 ((fl<? (flabs x) 0.75)
                  (fl/ (fl- (flexp-1 x) (flexp-1 (fl- x))) 2.0))
                 (else
                  (fl/ (fl- (flexp x) (flexp (fl- x))) 2.0))))))

(define flcosh
  (flop1 'flcosh
         (lambda (x)
           (cond ((not (flfinite? x)) (flabs x))
                 ((fl<? (flabs x) 0.75)
                  (fl+ 1.0 (fl/ (fl+ (flexp-1 x) (flexp-1 (fl- x))) 2.0)))
                 (else
                  (fl/ (fl+ (flexp x) (flexp (fl- x))) 2.0))))))

(define fltanh
  (flop1 'fltanh
         (lambda (x)
           (cond ((flinfinite? x) (flcopysign 1.0 x))
                 ((flnan? x) x)
                 (else
                  (let ((a (flsinh x))
                        (b (flcosh x)))
                    (cond ((fl=? a b)
                           1.0)
                          ((fl=? a (fl- b))
                           -1.0)
                          (else
                           (fl/ (flsinh x) (flcosh x))))))))))

;;; inverse hyperbolic functions

(define flasinh
  (flop1 'flasinh
         (lambda (x)
           (cond ((or (flinfinite? x)
                      (flnan? x))
                  x)
                 ((flnegative? x)
                  (fl- (flasinh (fl- x))))
                 ((fl<? x 3.725290298461914e-9)   ;; (flexpt 2. -28.)
                  x)
                 ((fl<? x 2.)
                  ;; the naive formula is
                  ;; (log (+ x (sqrt (+ (square x) 1))))

                  ;; We want to
                  ;; 1.  Use exact operations when possible (no roundoff)
                  ;; 2.  Add or subtract things of differing magnitudes,
                  ;;     so for most arguments at most one roundoff error.

                  ;; Biggeset possible problem near x=0, so we write
                  ;; (sqrt (+ 1 (square x)))
                  ;; as
                  ;; (+ 1 (- (sqrt (+ 1 (square x))) 1))
                  ;; and then multiply the second part in
                  ;; numerator and denominator by
                  ;; (+ (sqrt (+ 1 (square x))) 1)

                  (let ((x^2 (flsquare x)))
                    (fllog1+ (fl+ x
                                  (fl/ x^2
                                       (fl+ 1.
                                            (flsqrt (fl+ 1.0 x^2))))))))
                 ((fl<? x 268435456.) ;; (flexpt 2. 28.)
                  (let ((x^2 (flsquare x)))
                    (fllog (fl+ (fl* 2. x) ;; exact
                                ;; the rest is small
                                (fl/ 1.
                                     (fl+ x
                                          (flsqrt (fl+ 1.0 x^2))))))))
                 (else
                  (fl+ (fllog x) fl-log-2))))))

(define flacosh
  (flop1 'flacosh
         (lambda (x)
           (cond ((flnan? x) x)
                 ((fl<? x 1.0) +nan.0)
                 ((fl<? x 2.0)
                  ;; the naive formula is
                  ;; (log (+ x (sqrt (- (square x) 1))))

                  ;; We want to
                  ;; 1.  Use exact operations when possible (no roundoff)
                  ;; 2.  Add or subtract things of differing magnitudes,
                  ;;     so for most arguments at most one roundoff error.

                  (let ((x-1 (fl- x 1.))) ;; exact
                    (fllog1+ (fl+ x-1 ;; smaller than next expression
                                  (flsqrt (fl+ (fl* 2. x-1) ;; exact
                                               ;; relatively small
                                               (flsquare x-1)))))))
                 ((fl<? x 268435456.) ;; (flexpt 2. 28.)
                  (fllog (fl- (fl* 2. x) ;; exact
                              ;; next is smaller
                              (fl/ (fl+ x (flsqrt (fl* (fl- x 1.) ;; exact
                                                       (fl+ x 1.) ;; exact
                                                       )))))))
                 (else
                  (fl+ (fllog x) fl-log-2))))))

(define flatanh
  (flop1 'flatanh
         (lambda (x)
           (cond ((fl<? x 0.)
                  (fl- (flatanh (fl- x))))
                 ;; we rewrite
                 ;; (/ (+ 1 x) (- 1 x))
                 ;; as
                 ;; (+ 1 (* 2 (/ x (- 1 x))))
                 ;; and call fllog1+ instead of fllog
                 (else
                  (fl* +0.5                                    ;; exact
                       (fllog1+ (fl* +2.0                      ;; exact
                                     (fl/ x
                                          (fl- 1.0 x)))))))))) ;; exact

;;; Integer division

(define flquotient
  (flop2 'flquotient
         (lambda (x y)
           (fltruncate (fl/ x y)))))

;;; Copyright (C) 2026 Peter McGoron, contributed under the MIT license

(define (flremquo x y)
  (check-flonum! 'flremquo x)
  (check-flonum! 'flremquo y)
  (let ((ax (flabs x))
        (ay (flabs y)))
    (cond
      ((or (flzero? y) (not (flfinite? x)) (flnan? y))
       ;; Quotient is unspecified.
       (values (fl/ (fl* x y) (fl* x y)) 0))
      ((fl=? ax ay)
       ;; Simple case 1.
       (values (flcopysign 0.0 ax)
               (exact (fl* (flsgn ax) (flsgn ay)))))
      ((fl<? ax ay)
       ;; Simple case 2.
       ;; If ax/ay is less than 0.5, then return x as the remainder, and
       ;; 0 as the quotient. Otherwise, round up.
       (let ((div (fl/ ax ay)))
         (if (fl<=? div 0.5)
             (values x 0)
             (let ((q (flround div)))
               ;; These operations are exact, as `div` here can only
               ;; become 1.0 or -1.0, whose value is calculated exactly.
               (values (fl- x (fl* q y)) (exact q))))))
      (else (complicated-remquo x y)))))

(define (complicated-remquo x y)
  ;; Two radix-2 floating point numbers are represented as
  ;; 
  ;; x = 1.x_1x_2 ... × 2^e_x
  ;; y = 1.y_1y_2 ... × 2^e_y
  ;; 
  ;; To calculate x/y, we split the numbers into fractional and
  ;; exponential parts. (Slightly different than the representation
  ;; above.)
  (define-values (fx ex) (flnormalized-fraction-exponent x))
  (define-values (fy ey) (flnormalized-fraction-exponent y))
  ;; x = fx*2^{e_x}
  ;; y = fy*2^{e_y}
  ;; 
  ;; x/y = (fx/fy)*2^{e_x - e_y}
  ;; 
  ;; fx = 0.5 + fx_1/4 + fx_2/2^3 ...
  ;; fx = 0.5 + fy_1/4 + fy_2/2^3 ...
  ;; If we multiply this by the mantissa width (53 for doubles), then
  ;; we get (floating point) integers. We can then cast these to exact
  ;; integers.
  (define ix (exact (make-flonum fx precision-bits)))
  (define iy (exact (make-flonum fy precision-bits)))
  ;; Now we have
  ;; x/y = (ix/iy)*2^{e_x - e_y}
  ;; The mantissa exponent is cancelled out (we will need to add it to
  ;; the remainder later). Now we will do the actual integer division.
  ;; We can't do round/ here, because the rounding of the division here
  ;; might not reflect the final rounding. (Example: 5.75/2.0. You can
  ;; work it out on a piece of paper in a base-10 floating-point system.)
  ;; Hence we just use *an* integer division operator here, and truncate
  ;; is symmetric around the origin (like round is).
  (define-values (q r) (truncate/ ix iy))
  ;; x/y = (q + r/iy)*2^{e_x - e_y}
  ;;     = q*2^{e_x - e_y} + (r/iy)*2^{e_x - e_y}
  ;;
  ;; The exponent is still an issue. We can overflow if we multiply
  ;; by it, but more importantly, we can calculate the incorrect
  ;; remainder, because by multiplying r by 2 e_x - e_y times, we
  ;; may get `r` to be larger than `iy`. So we will have to modify
  ;; the quotient over time. Since r/iy is smaller than the quotient,
  ;; it won't cause the quotient to flip sign when we shift numbers
  ;; from the remainder to the quotient.
  ;;
  ;; The returned value has to have the appropriate sign, even
  ;; if the lower bits are all zero. `convert-sign` will return
  ;; the appropriately signed value. (It can be thought of as an
  ;; exact integer, wrapping version of copysign.)
  (define (convert-sign x sign)
    (cond
      ((and (negative? sign) (zero? x))
       -128)
      (else (* sign (abs x)))))
  (define (int-ldexp n exp)
    (if (>= exp 7)
        0
        (* n (expt 2 exp))))
  (define divsign
    (cond
      ((and (negative? r) (negative? iy)) 1)
      ((and (negative? r) (positive? iy)) -1)
      ((and (positive? r) (negative? iy)) -1)
      ((and (positive? r) (positive? iy)) 1)))
  (define (wrap x) (abs (remainder x 128)))
  ;; After the calculations below, the equation becomes (where r' < iy)
  ;;
  ;; x/y = q*2^{e_x - e_y} + I + r'/iy
  ;; x/y = q*2^{e_x - e_y} + I + r'*2^{e_y - 53}/iy*2^{e_y - 53}
  ;; x/y = q*2^{e_x - e_y} + I + r'*2^{e_y - 53}/y
  ;;
  ;; Using iy*2^{-53} = fx, and fx*2^{e_y} = y.
  ;;
  ;; Hence we incur one rounding, the one caused by make-flonum
  ;; (aka ldexp).
  (define q-sign (if (negative? q) -1 1))
  ;; First calculate the new quotient, only keeping 7 bits.
  (letrec ((add-power-of-two
            (lambda (q i)
              (wrap (+ q (* divsign (int-ldexp
                                     1
                                     (- ex ey i 1)))))))
;; Now calculate the remainder by incrementally multiplying it by
;; 2. When the numerator becomes larger in absolute value than the
;; denominator, then we need to shift that number, multiplied by
;; the appropriate power of 2, to the quotient.
;;
;; NOTE: Instead of fixup-remquo and return at the end, the loop
;; could keep the correct rounding at each step. This would make
;; things a little simpler.
           (loop
            (lambda (q r i)
              (if (>= i (- ex ey))
                (fixup-remquo q q-sign r ey)
                (let ((r (* r 2)))
                  (if (>= (abs r) (abs iy))
                      (loop (add-power-of-two q i)
                            (- r (* divsign iy))
                            (+ i 1))
                      (loop q r (+ i 1)))))))
           (fixup-remquo
            (lambda (q q-sign r ey)
              (let ((r*2 (* r 2)))
                (cond
                  ((or (> (abs r*2) (abs iy))
                       (and (= (abs r*2) (abs iy)) (odd? q)))
                   ;; Round up when remainder is above the halfway point,
                   ;; or at the halfway point and rounding up would make
                   ;; the quotient even.
                   (return (wrap (+ q (* divsign 1)))
                           q-sign
                           (- r (* divsign iy))
                           ey))
                  (else (return q q-sign r ey))))))
           (return (lambda (q q-sign r ey)
                     (values (make-flonum (flonum r) (- ey 53))
                             (convert-sign q q-sign)))))
    (let ((q (int-ldexp (abs q) (- ex ey))))
      (if (zero? r)
          (values (flcopysign 0.0 y) q)
          (loop q r 0)))))

(define (fltruncate-remainder x y)
  (let ((ax (flabs x))
        (ay (flabs y)))
    (cond
      ((or (flzero? y) (not (flfinite? x)) (flnan? y))
       (fl/ (fl* x y) (fl* x y)))
      ((fl=? ax ay) (flcopysign 0.0 ax))
      ((fl<? ax ay)
       ;; truncate(|x/y|) = 0, so r = x.
       ax)
      (else (fltruncate-remainder* x y)))))

(define (fltruncate-remainder* x y)
  ;; See flremquo. This code is much simpler as the rounding rule is
  ;; simpler.
  (let*-values (((fx ex) (flnormalized-fraction-exponent x))
                ((fy ey) (flnormalized-fraction-exponent y))
                ((ix) (exact (make-flonum fx precision-bits)))
                ((iy) (exact (make-flonum fy precision-bits)))
                ((r) (truncate-remainder ix iy))
                ((adjustment-sign) (cond
                                     ((and (negative? r) (positive? iy))
                                      (flonum -1.0))
                                     ((and (positive? r) (negative? iy))
                                      (flonum -1.0))
                                     ((and (positive? r) (positive? iy))
                                      (flonum 1.0))
                                     ((and (negative? r) (negative? iy))
                                      (flonum 1.0)))))
    (if (zero? r)
        (flonum r)
        (do ((n 0 (+ n 1))
             (r r (let ((r (* r 2)))
                    (if (>= (abs r) (abs iy))
                        (- r (* adjustment-sign iy))
                        r))))
            ((= n (- ex ey))
             (make-flonum (flonum r) (- ey 53)))))))

(define flremainder
  (flop2 'flremainder fltruncate-remainder))

;; Special functions are defined in 144.special.scm

; eof
