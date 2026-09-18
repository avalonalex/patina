;;; SRFI 4: homogeneous numeric vector datatypes.
;;;
;;; Patina-authored, over `(r6rs bytevectors)`. The SRFI's own distribution
;;; carries John Cowan's portable R7RS port, and it was measured working here
;;; (240 of 240 on its suite, both backends) before being set aside: none of
;;; its four files carries a licence notice, and a sibling file in the same
;;; `contrib/cowan/` directory is under William D Clinger's terms rather than
;;; the repository's MIT, so the directory is demonstrably not covered
;;; uniformly by that header. Rather than ship files whose licence we had
;;; inferred, this implements the SRFI's interface instead — an API is not the
;;; licensed artifact, the same reasoning `lib/srfi/PROVENANCE.md` records for
;;; the SRFI 14 rewrite.
;;;
;;; Representation: a record wrapping a bytevector and a tag symbol. The tag
;;; is what makes the ten types *distinct* — without it `u8vector?` could not
;;; refuse an `s8vector`, since both are a bytevector of the same length, and
;;; SRFI 4 requires the predicates to be exclusive. Elements are stored in
;;; native endianness, which is what `(r6rs bytevectors)`' `-native-` variants
;;; mean; `u8` and `s8` have no endianness and use the plain accessors.
;;;
;;; `u8vector` is the one type that is *not* wrapped: it is a bytevector
;;; outright. R7RS already gives bytevectors the exact semantics SRFI 4 asks
;;; of a u8vector, `(scheme base)`'s bytevector procedures then apply to one,
;;; and `(srfi 160 base)` builds on that identity. The cost is that
;;; `u8vector?` and `bytevector?` accept each other, which SRFI 4 neither
;;; requires nor forbids; `tests/scheme/srfi/homogeneous-vectors.scm` asserts
;;; it so the choice is visible rather than implicit.
;;;
;;; `(srfi 160 base)` depends on this beyond the SRFI's own text: its complex
;;; vectors wrap an `f32vector`/`f64vector` and drive them through
;;; `make-f32vector`, `f32vector-set!` and `f32vector-length`, so those must
;;; work on a vector of any length, including one built for a c64vector.

(define-record-type <hvector>
  (%make-hvector tag bytes)
  %hvector?
  (tag %hvector-tag)
  (bytes %hvector-bytes))

;;; The u8 case is special, as the header explains: a u8vector is a plain
;;; bytevector, so these three have to see through both representations.

(define (%is? obj tag)
  (if (eq? tag 'u8)
      (bytevector? obj)
      (and (%hvector? obj) (eq? (%hvector-tag obj) tag))))

(define (%bytes v tag who)
  (cond ((and (eq? tag 'u8) (bytevector? v)) v)
        ((and (%hvector? v) (eq? (%hvector-tag v) tag)) (%hvector-bytes v))
        (else (error "not a vector of the expected type" who tag v))))

(define (%make tag size len who)
  (if (not (and (exact? len) (integer? len) (>= len 0)))
      (error "length must be an exact non-negative integer" who len))
  (let ((bytes (make-bytevector (* len size) 0)))
    (if (eq? tag 'u8) bytes (%make-hvector tag bytes))))

;;; An index is checked here rather than left to the bytevector accessor, so
;;; the error names the element index the caller passed rather than the byte
;;; offset it became.
(define (%offset i size bytes who)
  (let ((len (quotient (bytevector-length bytes) size)))
    (if (not (and (exact? i) (integer? i) (>= i 0) (< i len)))
        (error "index out of range" who i len))
    (* i size)))

;;; The element ranges, checked on every store.
;;;
;;; Without this the ten types disagree about a bad value: `bytevector-u8-set!`
;;; range-checks, so `(u8vector-set! v 0 300)` raised, while every wider type
;;; bottoms out in a `-native-set!` that silently truncates —
;;; `(s8vector-set! v 0 200)` stored -56 and `(u16vector-set! v 0 70000)`
;;; stored 4464. SRFI 4 leaves an out-of-range store unspecified, so either
;;; answer is conforming; what is not defensible is *both*, inside one
;;; library, so that a program retargeted from u8 to s8 loses its error.
;;; Raising is the choice, because the alternative is silent data corruption.
;;;
;;; The bounds are the same ones `(srfi 160 base)`'s own `valid.scm` states,
;;; which ships those predicates and never calls them.
(define (%check-value value tag who)
  (define (int-in? lo hi)
    (and (exact? value) (integer? value) (<= lo value hi)))
  (if (not (case tag
             ((u8) (int-in? 0 255))
             ((s8) (int-in? -128 127))
             ((u16) (int-in? 0 65535))
             ((s16) (int-in? -32768 32767))
             ((u32) (int-in? 0 4294967295))
             ((s32) (int-in? -2147483648 2147483647))
             ((u64) (int-in? 0 18446744073709551615))
             ((s64) (int-in? -9223372036854775808 9223372036854775807))
             ;; The float types take any real; the accessor rounds to the
             ;; stored precision, which is what f32 storage means.
             ((f32 f64) (real? value))
             (else #f)))
      (error "value out of range for element type" who tag value))
  value)

(define (%fill! v value len-of set!-of)
  (let ((len (len-of v)))
    (let loop ((i 0))
      (if (< i len)
          (begin (set!-of v i value) (loop (+ i 1)))))))

(define (%from-list tag size lst set!-of who)
  (let* ((len (length lst))
         (v (%make tag size len who)))
    (let loop ((i 0) (rest lst))
      (if (pair? rest)
          (begin (set!-of v i (car rest))
                 (loop (+ i 1) (cdr rest)))))
    v))

;;; SRFI 4's `->list` takes optional start and end, defaulting to the whole
;;; vector. `(srfi 160 base)` overrides these with its own bounded versions,
;;; but they are part of the SRFI's interface and callers of `(srfi 4)` alone
;;; get them here.
(define (%->list v range len-of ref-of who)
  (let* ((len (len-of v))
         (start (if (pair? range) (car range) 0))
         (end (if (and (pair? range) (pair? (cdr range))) (cadr range) len)))
    (if (not (and (exact? start) (integer? start) (<= 0 start len)))
        (error "start out of range" who start len))
    (if (not (and (exact? end) (integer? end) (<= start end len)))
        (error "end out of range" who end len))
    (let loop ((i (- end 1)) (acc '()))
      (if (< i start)
          acc
          (loop (- i 1) (cons (ref-of v i) acc))))))

;;; u8vector — each element is an exact integer in 0..255.

(define (make-u8vector len . fill)
  (let ((v (%make 'u8 1 len 'make-u8vector)))
    (if (pair? fill)
        (%fill! v (car fill) u8vector-length u8vector-set!))
    v))

(define (u8vector . elements)
  (%from-list 'u8 1 elements u8vector-set! 'u8vector))

(define (u8vector? obj) (%is? obj 'u8))

(define (u8vector-length v)
  (quotient (bytevector-length (%bytes v 'u8 'u8vector-length)) 1))

(define (u8vector-ref v i)
  (let ((b (%bytes v 'u8 'u8vector-ref)))
    (bytevector-u8-ref b (%offset i 1 b 'u8vector-ref))))

(define (u8vector-set! v i value)
  (let ((b (%bytes v 'u8 'u8vector-set!)))
    (bytevector-u8-set! b (%offset i 1 b 'u8vector-set!)
       (%check-value value 'u8 'u8vector-set!))))

(define (u8vector->list v . range)
  (%->list v range u8vector-length u8vector-ref 'u8vector->list))

(define (list->u8vector lst)
  (%from-list 'u8 1 lst u8vector-set! 'list->u8vector))

;;; s8vector — each element is an exact integer in -128..127.

(define (make-s8vector len . fill)
  (let ((v (%make 's8 1 len 'make-s8vector)))
    (if (pair? fill)
        (%fill! v (car fill) s8vector-length s8vector-set!))
    v))

(define (s8vector . elements)
  (%from-list 's8 1 elements s8vector-set! 's8vector))

(define (s8vector? obj) (%is? obj 's8))

(define (s8vector-length v)
  (quotient (bytevector-length (%bytes v 's8 's8vector-length)) 1))

(define (s8vector-ref v i)
  (let ((b (%bytes v 's8 's8vector-ref)))
    (bytevector-s8-ref b (%offset i 1 b 's8vector-ref))))

(define (s8vector-set! v i value)
  (let ((b (%bytes v 's8 's8vector-set!)))
    (bytevector-s8-set! b (%offset i 1 b 's8vector-set!)
       (%check-value value 's8 's8vector-set!))))

(define (s8vector->list v . range)
  (%->list v range s8vector-length s8vector-ref 's8vector->list))

(define (list->s8vector lst)
  (%from-list 's8 1 lst s8vector-set! 'list->s8vector))

;;; u16vector — each element is an exact integer in 0..65535.

(define (make-u16vector len . fill)
  (let ((v (%make 'u16 2 len 'make-u16vector)))
    (if (pair? fill)
        (%fill! v (car fill) u16vector-length u16vector-set!))
    v))

(define (u16vector . elements)
  (%from-list 'u16 2 elements u16vector-set! 'u16vector))

(define (u16vector? obj) (%is? obj 'u16))

(define (u16vector-length v)
  (quotient (bytevector-length (%bytes v 'u16 'u16vector-length)) 2))

(define (u16vector-ref v i)
  (let ((b (%bytes v 'u16 'u16vector-ref)))
    (bytevector-u16-native-ref b (%offset i 2 b 'u16vector-ref))))

(define (u16vector-set! v i value)
  (let ((b (%bytes v 'u16 'u16vector-set!)))
    (bytevector-u16-native-set! b (%offset i 2 b 'u16vector-set!)
       (%check-value value 'u16 'u16vector-set!))))

(define (u16vector->list v . range)
  (%->list v range u16vector-length u16vector-ref 'u16vector->list))

(define (list->u16vector lst)
  (%from-list 'u16 2 lst u16vector-set! 'list->u16vector))

;;; s16vector — each element is an exact integer in -32768..32767.

(define (make-s16vector len . fill)
  (let ((v (%make 's16 2 len 'make-s16vector)))
    (if (pair? fill)
        (%fill! v (car fill) s16vector-length s16vector-set!))
    v))

(define (s16vector . elements)
  (%from-list 's16 2 elements s16vector-set! 's16vector))

(define (s16vector? obj) (%is? obj 's16))

(define (s16vector-length v)
  (quotient (bytevector-length (%bytes v 's16 's16vector-length)) 2))

(define (s16vector-ref v i)
  (let ((b (%bytes v 's16 's16vector-ref)))
    (bytevector-s16-native-ref b (%offset i 2 b 's16vector-ref))))

(define (s16vector-set! v i value)
  (let ((b (%bytes v 's16 's16vector-set!)))
    (bytevector-s16-native-set! b (%offset i 2 b 's16vector-set!)
       (%check-value value 's16 's16vector-set!))))

(define (s16vector->list v . range)
  (%->list v range s16vector-length s16vector-ref 's16vector->list))

(define (list->s16vector lst)
  (%from-list 's16 2 lst s16vector-set! 'list->s16vector))

;;; u32vector — each element is an exact integer in 0..2^32-1.

(define (make-u32vector len . fill)
  (let ((v (%make 'u32 4 len 'make-u32vector)))
    (if (pair? fill)
        (%fill! v (car fill) u32vector-length u32vector-set!))
    v))

(define (u32vector . elements)
  (%from-list 'u32 4 elements u32vector-set! 'u32vector))

(define (u32vector? obj) (%is? obj 'u32))

(define (u32vector-length v)
  (quotient (bytevector-length (%bytes v 'u32 'u32vector-length)) 4))

(define (u32vector-ref v i)
  (let ((b (%bytes v 'u32 'u32vector-ref)))
    (bytevector-u32-native-ref b (%offset i 4 b 'u32vector-ref))))

(define (u32vector-set! v i value)
  (let ((b (%bytes v 'u32 'u32vector-set!)))
    (bytevector-u32-native-set! b (%offset i 4 b 'u32vector-set!)
       (%check-value value 'u32 'u32vector-set!))))

(define (u32vector->list v . range)
  (%->list v range u32vector-length u32vector-ref 'u32vector->list))

(define (list->u32vector lst)
  (%from-list 'u32 4 lst u32vector-set! 'list->u32vector))

;;; s32vector — each element is an exact integer in -2^31..2^31-1.

(define (make-s32vector len . fill)
  (let ((v (%make 's32 4 len 'make-s32vector)))
    (if (pair? fill)
        (%fill! v (car fill) s32vector-length s32vector-set!))
    v))

(define (s32vector . elements)
  (%from-list 's32 4 elements s32vector-set! 's32vector))

(define (s32vector? obj) (%is? obj 's32))

(define (s32vector-length v)
  (quotient (bytevector-length (%bytes v 's32 's32vector-length)) 4))

(define (s32vector-ref v i)
  (let ((b (%bytes v 's32 's32vector-ref)))
    (bytevector-s32-native-ref b (%offset i 4 b 's32vector-ref))))

(define (s32vector-set! v i value)
  (let ((b (%bytes v 's32 's32vector-set!)))
    (bytevector-s32-native-set! b (%offset i 4 b 's32vector-set!)
       (%check-value value 's32 's32vector-set!))))

(define (s32vector->list v . range)
  (%->list v range s32vector-length s32vector-ref 's32vector->list))

(define (list->s32vector lst)
  (%from-list 's32 4 lst s32vector-set! 'list->s32vector))

;;; u64vector — each element is an exact integer in 0..2^64-1.

(define (make-u64vector len . fill)
  (let ((v (%make 'u64 8 len 'make-u64vector)))
    (if (pair? fill)
        (%fill! v (car fill) u64vector-length u64vector-set!))
    v))

(define (u64vector . elements)
  (%from-list 'u64 8 elements u64vector-set! 'u64vector))

(define (u64vector? obj) (%is? obj 'u64))

(define (u64vector-length v)
  (quotient (bytevector-length (%bytes v 'u64 'u64vector-length)) 8))

(define (u64vector-ref v i)
  (let ((b (%bytes v 'u64 'u64vector-ref)))
    (bytevector-u64-native-ref b (%offset i 8 b 'u64vector-ref))))

(define (u64vector-set! v i value)
  (let ((b (%bytes v 'u64 'u64vector-set!)))
    (bytevector-u64-native-set! b (%offset i 8 b 'u64vector-set!)
       (%check-value value 'u64 'u64vector-set!))))

(define (u64vector->list v . range)
  (%->list v range u64vector-length u64vector-ref 'u64vector->list))

(define (list->u64vector lst)
  (%from-list 'u64 8 lst u64vector-set! 'list->u64vector))

;;; s64vector — each element is an exact integer in -2^63..2^63-1.

(define (make-s64vector len . fill)
  (let ((v (%make 's64 8 len 'make-s64vector)))
    (if (pair? fill)
        (%fill! v (car fill) s64vector-length s64vector-set!))
    v))

(define (s64vector . elements)
  (%from-list 's64 8 elements s64vector-set! 's64vector))

(define (s64vector? obj) (%is? obj 's64))

(define (s64vector-length v)
  (quotient (bytevector-length (%bytes v 's64 's64vector-length)) 8))

(define (s64vector-ref v i)
  (let ((b (%bytes v 's64 's64vector-ref)))
    (bytevector-s64-native-ref b (%offset i 8 b 's64vector-ref))))

(define (s64vector-set! v i value)
  (let ((b (%bytes v 's64 's64vector-set!)))
    (bytevector-s64-native-set! b (%offset i 8 b 's64vector-set!)
       (%check-value value 's64 's64vector-set!))))

(define (s64vector->list v . range)
  (%->list v range s64vector-length s64vector-ref 's64vector->list))

(define (list->s64vector lst)
  (%from-list 's64 8 lst s64vector-set! 'list->s64vector))

;;; f32vector — each element is an inexact real, stored at single precision.

(define (make-f32vector len . fill)
  (let ((v (%make 'f32 4 len 'make-f32vector)))
    (if (pair? fill)
        (%fill! v (car fill) f32vector-length f32vector-set!))
    v))

(define (f32vector . elements)
  (%from-list 'f32 4 elements f32vector-set! 'f32vector))

(define (f32vector? obj) (%is? obj 'f32))

(define (f32vector-length v)
  (quotient (bytevector-length (%bytes v 'f32 'f32vector-length)) 4))

(define (f32vector-ref v i)
  (let ((b (%bytes v 'f32 'f32vector-ref)))
    (bytevector-ieee-single-native-ref b (%offset i 4 b 'f32vector-ref))))

(define (f32vector-set! v i value)
  (let ((b (%bytes v 'f32 'f32vector-set!)))
    (bytevector-ieee-single-native-set! b (%offset i 4 b 'f32vector-set!)
       (%check-value value 'f32 'f32vector-set!))))

(define (f32vector->list v . range)
  (%->list v range f32vector-length f32vector-ref 'f32vector->list))

(define (list->f32vector lst)
  (%from-list 'f32 4 lst f32vector-set! 'list->f32vector))

;;; f64vector — each element is an inexact real, stored at double precision.

(define (make-f64vector len . fill)
  (let ((v (%make 'f64 8 len 'make-f64vector)))
    (if (pair? fill)
        (%fill! v (car fill) f64vector-length f64vector-set!))
    v))

(define (f64vector . elements)
  (%from-list 'f64 8 elements f64vector-set! 'f64vector))

(define (f64vector? obj) (%is? obj 'f64))

(define (f64vector-length v)
  (quotient (bytevector-length (%bytes v 'f64 'f64vector-length)) 8))

(define (f64vector-ref v i)
  (let ((b (%bytes v 'f64 'f64vector-ref)))
    (bytevector-ieee-double-native-ref b (%offset i 8 b 'f64vector-ref))))

(define (f64vector-set! v i value)
  (let ((b (%bytes v 'f64 'f64vector-set!)))
    (bytevector-ieee-double-native-set! b (%offset i 8 b 'f64vector-set!)
       (%check-value value 'f64 'f64vector-set!))))

(define (f64vector->list v . range)
  (%->list v range f64vector-length f64vector-ref 'f64vector->list))

(define (list->f64vector lst)
  (%from-list 'f64 8 lst f64vector-set! 'list->f64vector))
