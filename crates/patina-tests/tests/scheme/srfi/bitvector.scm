;; `(patina bitvector)` — bit vectors, one bit per element.
;;
;; Patina's own, written for `(srfi 231)`'s `u1-storage-class`, which SRFI 231
;; exports publicly and builds from six `u1vector-*` procedures it expects the
;; host to supply. Those are not SRFI 160's — its twelve types begin at u8 —
;; they are a chibi C extension, so an implementation bundling SRFI 231 has to
;; provide them itself. See `lib/patina/bitvector.sld` for why the name is not
;; published as `(srfi 160 u1)`.
;;
;; The rows below are about the *packing*, because that is the part a
;; plausible implementation gets wrong. Eight elements share a byte, so every
;; operation has to isolate one bit without disturbing its seven neighbours,
;; and the failure mode is not a crash — it is a neighbouring element quietly
;; changing. `(srfi 231)`'s own suite exercises this library only through
;; `u1-storage-class`, and only shallowly.

(import (scheme base) (scheme write) (patina bitvector) (srfi 64))

(test-begin "bitvector")

;; ---------------------------------------------------------------------------
;; The packing: a write must not disturb its neighbours
;; ---------------------------------------------------------------------------

(test-equal "setting one bit leaves the rest of its byte alone"
  '(0 0 0 1 0 0 0 0)
  (let ((v (make-u1vector 8 0)))
    (u1vector-set! v 3 1)
    (u1vector->list v)))

(test-equal "clearing one bit leaves the rest of its byte alone"
  '(1 1 1 0 1 1 1 1)
  (let ((v (make-u1vector 8 1)))
    (u1vector-set! v 3 0)
    (u1vector->list v)))

;; The byte boundary is where an off-by-one in the shift or the divisor shows.
(test-equal "bits 7, 8 and 9 are independent across the byte boundary"
  '(0 1 0)
  (let ((v (make-u1vector 10 0)))
    (u1vector-set! v 8 1)
    (list (u1vector-ref v 7) (u1vector-ref v 8) (u1vector-ref v 9))))

(test-equal "every bit of a byte can be set independently"
  '(1 1 1 1 1 1 1 1)
  (let ((v (make-u1vector 8 0)))
    (let loop ((i 0))
      (if (< i 8) (begin (u1vector-set! v i 1) (loop (+ i 1)))))
    (u1vector->list v)))

;; A length that is not a multiple of 8 must not read the padding bits of its
;; last byte as elements.
(test-equal "a length that is not a byte multiple has exactly that length" 9
  (u1vector-length (make-u1vector 9 1)))
(test-equal "a fill does not leak the last byte's padding into the length" 9
  (length (u1vector->list (make-u1vector 9 1))))

;; ---------------------------------------------------------------------------
;; Construction and conversion
;; ---------------------------------------------------------------------------

(test-equal "make-u1vector defaults to zeros" '(0 0 0)
  (u1vector->list (make-u1vector 3)))
(test-equal "make-u1vector fills with ones" '(1 1 1)
  (u1vector->list (make-u1vector 3 1)))
(test-equal "an empty bit vector is empty" '() (u1vector->list (make-u1vector 0)))
(test-equal "u1vector takes its elements" '(1 0 1)
  (u1vector->list (u1vector 1 0 1)))
(test-equal "list->u1vector round-trips through ->list" '(1 0 0 1 1 0 1)
  (u1vector->list (list->u1vector '(1 0 0 1 1 0 1))))

;; ---------------------------------------------------------------------------
;; The type is distinct
;; ---------------------------------------------------------------------------
;; A bare bytevector must not pass as a bit vector: the length in *bits* is
;; not recoverable from a byte count, which is why this is a record.

(test-assert "a bit vector is one" (u1vector? (make-u1vector 3)))
(test-assert "a bytevector is not a bit vector"
  (not (u1vector? (bytevector 1 2 3))))
(test-assert "a vector is not a bit vector" (not (u1vector? (vector 1))))

;; `u1?` is the element predicate SRFI 231's storage class checks with.
(test-assert "0 is an element" (u1? 0))
(test-assert "1 is an element" (u1? 1))
(test-assert "2 is not" (not (u1? 2)))
(test-assert "-1 is not" (not (u1? -1)))
(test-assert "1.0 is not, being inexact" (not (u1? 1.0)))
(test-assert "a symbol is not" (not (u1? 'one)))

;; ---------------------------------------------------------------------------
;; Refusals
;; ---------------------------------------------------------------------------

(define (refuses? thunk) (guard (e (#t #t)) (thunk) #f))

(test-assert "an index at the length is refused"
  (refuses? (lambda () (u1vector-ref (make-u1vector 8 0) 8))))
(test-assert "a negative index is refused"
  (refuses? (lambda () (u1vector-ref (make-u1vector 8 0) -1))))
(test-assert "an index beyond the last byte is refused"
  ;; 9 elements occupy two bytes, so index 9..15 exist in storage but not in
  ;; the vector — the check has to use the bit length, not the byte count.
  (refuses? (lambda () (u1vector-ref (make-u1vector 9 0) 12))))
(test-assert "storing 2 is refused"
  (refuses? (lambda () (u1vector-set! (make-u1vector 8 0) 0 2))))
(test-assert "an inexact element is refused"
  (refuses? (lambda () (u1vector-set! (make-u1vector 8 0) 0 1.0))))
(test-assert "a negative length is refused"
  (refuses? (lambda () (make-u1vector -1))))
(test-assert "a fill that is not a bit is refused"
  (refuses? (lambda () (make-u1vector 3 7))))

;; ---------------------------------------------------------------------------
;; At a size where packing matters
;; ---------------------------------------------------------------------------

(test-assert "1000 alternating bits read back correctly"
  (let ((v (make-u1vector 1000 0)))
    (let loop ((i 0))
      (if (< i 1000)
          (begin (if (even? i) (u1vector-set! v i 1)) (loop (+ i 1)))))
    (let check ((i 0))
      (cond ((>= i 1000) #t)
            ((= (u1vector-ref v i) (if (even? i) 1 0)) (check (+ i 1)))
            (else #f)))))

(test-end)
