;;; SRFI 151 derived operations.
;;;
;;; Ported from the reference implementation by Alex Shinn (BSD-style licence,
;;; http://synthcode.com/license.txt), with one adaptation: bitwise-and,
;;; bitwise-ior, bitwise-xor and bitwise-not are Rust primitives here rather
;;; than being folded up from binary operators in Scheme, so the reference's
;;; `make-nary` / `bitwise-complement` scaffolding is gone and the operators
;;; below call the n-ary primitives directly.

(define (bitwise-eqv . args)
  (bitwise-not (apply bitwise-xor -1 args)))
(define (bitwise-nand i j) (bitwise-not (bitwise-and i j)))
(define (bitwise-nor i j) (bitwise-not (bitwise-ior i j)))

(define (bitwise-andc1 i j) (bitwise-and (bitwise-not i) j))
(define (bitwise-andc2 i j) (bitwise-and i (bitwise-not j)))
(define (bitwise-orc1 i j) (bitwise-ior (bitwise-not i) j))
(define (bitwise-orc2 i j) (bitwise-ior i (bitwise-not j)))

(define (any-bit-set? test-bits i)
  (not (zero? (bitwise-and test-bits i))))
(define (every-bit-set? test-bits i)
  (= test-bits (bitwise-and test-bits i)))

(define (first-set-bit i)
  (if (zero? i)
      -1
      (- (integer-length (- i (bitwise-and i (- i 1)))) 1)))

(define (mask len)
  (- (arithmetic-shift 1 len) 1))

(define (range start end)
  (arithmetic-shift (mask (- end start)) start))

(define (bitwise-if mask m n)
  (bitwise-ior (bitwise-and mask m)
               (bitwise-and (bitwise-not mask) n)))

(define (bit-field n start end)
  (bitwise-and (arithmetic-shift n (- start)) (mask (- end start))))

(define (bit-field-any? n start end)
  (not (zero? (bitwise-and (arithmetic-shift n (- start))
                           (mask (- end start))))))

(define (bit-field-every? n start end)
  (let ((lo (mask (- end start))))
    (= (bitwise-and lo (arithmetic-shift n (- start))) lo)))

(define (bit-field-replace-same dst src start end)
  (bitwise-if (range start end) src dst))

(define (bit-field-replace dst src start end)
  (bit-field-replace-same dst (arithmetic-shift src start) start end))

(define (copy-bit index i boolean)
  (bit-field-replace i (if boolean 1 0) index (+ index 1)))

(define (bit-swap i1 i2 i)
  (let ((b1 (bit-set? i1 i))
        (b2 (bit-set? i2 i)))
    (copy-bit i2 (copy-bit i1 i b2) b1)))

(define (bit-field-clear n start end)
  (bit-field-replace n 0 start end))

(define (bit-field-set n start end)
  (bitwise-ior n (range start end)))

(define (bit-field-rotate n count start end)
  (let* ((width (- end start))
         (count (modulo count width))
         (m (bitwise-not (arithmetic-shift -1 width)))
         (n^ (bitwise-and m (arithmetic-shift n (- start)))))
    (bitwise-ior (arithmetic-shift
                  (bitwise-ior (bitwise-and m (arithmetic-shift n^ count))
                               (arithmetic-shift n^ (- count width)))
                  start)
                 (bitwise-and (bitwise-not (arithmetic-shift m start)) n))))

(define (bit-reverse n len)
  (let lp ((n n) (i 1) (res 0))
    (if (> i len)
        res
        (lp (arithmetic-shift n -1)
            (+ i 1)
            (bitwise-ior (arithmetic-shift res 1)
                         (bitwise-and n 1))))))

(define (bit-field-reverse i start end)
  (bitwise-if (range start end)
              (arithmetic-shift (bit-reverse (bit-field i start end)
                                             (- end start))
                                start)
              i))

(define (vector->bits vec)
  (let ((len (vector-length vec)))
    (let lp ((i 0) (exp 1) (res 0))
      (cond
       ((= i len) res)
       ((vector-ref vec i) (lp (+ i 1) (* exp 2) (+ res exp)))
       (else (lp (+ i 1) (* exp 2) res))))))

(define (bits->vector n . o)
  (let* ((len (if (pair? o) (car o) (integer-length n)))
         (res (make-vector len #f)))
    (let lp ((n n) (i 0))
      (cond
       ((>= i len) res)
       (else
        (if (odd? n) (vector-set! res i #t))
        (lp (arithmetic-shift n -1) (+ i 1)))))))

(define (list->bits ls) (vector->bits (list->vector ls)))
(define (bits->list n . o) (vector->list (apply bits->vector n o)))
(define (bits . o) (list->bits o))

(define (bitwise-fold kons knil i)
  (let lp ((i i) (acc knil))
    (if (zero? i)
        acc
        (lp (arithmetic-shift i -1) (kons (odd? i) acc)))))

(define (bitwise-for-each proc i)
  (bitwise-fold (lambda (b acc) (proc b)) #f i))

(define (bitwise-unfold stop? mapper successor seed)
  (let lp ((state seed) (exp 1) (i 0))
    (if (stop? state)
        i
        (lp (successor state)
            (* exp 2)
            (if (mapper state) (+ i exp) i)))))

(define (make-bitwise-generator i)
  (lambda ()
    (let ((res (odd? i)))
      (set! i (arithmetic-shift i -1))
      res)))
