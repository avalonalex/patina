;;; Bit vectors: eight elements per byte of a bytevector.
;;;
;;; A bit vector is a record rather than a bare bytevector, for two reasons.
;;; The length in *bits* is not recoverable from the byte count — a 9-bit and
;;; a 16-bit vector both need two bytes — and `u1vector?` has to be able to
;;; refuse a plain bytevector, which it could not if the two were the same
;;; type. That mirrors how `(srfi 4)`'s wider types are tagged records here
;;; for the same reason.

(define-record-type <u1vector>
  (%make-u1vector length bytes)
  u1vector?
  (length u1vector-length)
  (bytes %u1vector-bytes))

;;; The number of bytes needed to hold `n` bits.
(define (%byte-count n)
  (quotient (+ n 7) 8))

(define (%check-index v i who)
  (let ((len (u1vector-length v)))
    (if (not (and (exact? i) (integer? i) (>= i 0) (< i len)))
        (error "index out of range" who i len))))

;;; An element is a bit: 0 or 1. SRFI 231's `u1?` is the storage class's
;;; element check, so it decides what may be stored.
(define (u1? x)
  (and (exact? x) (integer? x) (or (= x 0) (= x 1))))

(define (make-u1vector n . fill)
  (if (not (and (exact? n) (integer? n) (>= n 0)))
      (error "length must be an exact non-negative integer" 'make-u1vector n))
  (let ((bit (if (pair? fill) (car fill) 0)))
    (if (not (u1? bit))
        (error "fill must be 0 or 1" 'make-u1vector bit))
    ;; Every byte is all-zeros or all-ones, so a fill needs no per-bit work.
    (%make-u1vector n (make-bytevector (%byte-count n) (if (= bit 0) 0 255)))))

(define (u1vector . bits) (list->u1vector bits))

(define (u1vector-ref v i)
  (%check-index v i 'u1vector-ref)
  (let ((byte (bytevector-u8-ref (%u1vector-bytes v) (quotient i 8))))
    ;; Bit 0 of a byte is element 0 of that byte — little-endian within the
    ;; byte. Nothing observable depends on the choice, since the bytes are
    ;; never exposed, but it has to be the same in `ref` and `set!`.
    (bitwise-and (arithmetic-shift byte (- (remainder i 8))) 1)))

(define (u1vector-set! v i bit)
  (%check-index v i 'u1vector-set!)
  (if (not (u1? bit))
      (error "element must be 0 or 1" 'u1vector-set! bit))
  (let* ((bytes (%u1vector-bytes v))
         (at (quotient i 8))
         (mask (arithmetic-shift 1 (remainder i 8)))
         (byte (bytevector-u8-ref bytes at)))
    (bytevector-u8-set! bytes at
                        (if (= bit 0)
                            (bitwise-and byte (bitwise-and 255 (bitwise-not mask)))
                            (bitwise-ior byte mask)))))

(define (u1vector->list v)
  (let loop ((i (- (u1vector-length v) 1)) (acc '()))
    (if (< i 0)
        acc
        (loop (- i 1) (cons (u1vector-ref v i) acc)))))

(define (list->u1vector bits)
  (let* ((n (length bits))
         (v (make-u1vector n)))
    (let loop ((i 0) (rest bits))
      (if (pair? rest)
          (begin (u1vector-set! v i (car rest))
                 (loop (+ i 1) (cdr rest)))))
    v))
