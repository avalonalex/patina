;; SRFI 4 and SRFI 160 homogeneous numeric vectors.
;;
;; Two things this file exists for, because neither is covered elsewhere.
;;
;; **SRFI 4's own suite cannot be registered.** It is a print-only harness: it
;; displays "OK" or "FAIL" per assertion and then exits 0 either way, with no
;; counter and no exit status a driver can read. Registering it in
;; `upstream_srfi_suites.rs` would be vacuously green — the same failure mode
;; `SRFI_64_BODY` exists to prevent there. So the coverage is here instead,
;; under a framework that reports.
;;
;; **SRFI 160's registered suite tests one type of twelve.** Upstream says why
;; in its own comment — "if one vector type works, they all work", since the
;; twelve libraries are `sed`-expanded from a single template, so a template
;; defect shows in all of them. That argument is sound for the *template*, and
;; it is silent about the per-type parameters the expansion substitutes: the
;; element width, the signedness, and the accessor each type bottoms out in.
;; A u8vector that accepted 300, or an s8vector that read -1 as 255, would
;; pass a suite that only ever looked at s16. So every type gets rows here.
;;
;; SRFI 4 does not say what storing 300 in a u8vector does, so no row here
;; asserts Patina's answer against another implementation's. What the rows
;; below *do* pin is that the answer is the **same across the ten types**.
;; That distinction is the reason they exist: until #383's review, it was not.
;; `bytevector-u8-set!` range-checks and the wider `-native-set!` variants do
;; not, so `(u8vector-set! v 0 300)` raised while `(s8vector-set! v 0 200)`
;; silently stored -56 and `(u16vector-set! v 0 70000)` stored 4464 — a
;; program retargeted from u8 to s8 lost its error and got corruption. Either
;; answer conforms; both at once, inside one library, does not.

;; `(srfi 160 base)` has the constructors, predicates and accessors for all
;; twelve types; the map/fold/append family lives in the *per-type* libraries,
;; one per element type. Both are imported, and the per-type ones only for the
;; types whose operations are exercised below.
(import (scheme base) (scheme write) (scheme complex) (srfi 64)
        (srfi 4)
        (srfi 160 base)
        (srfi 160 u8) (srfi 160 s8) (srfi 160 u16) (srfi 160 s16)
        (srfi 160 u32)
        (srfi 160 s32) (srfi 160 u64) (srfi 160 s64)
        (srfi 160 f32) (srfi 160 f64) (srfi 128)
        ;; The two complex types have no SRFI 4 layer under them — they wrap
        ;; an f32vector/f64vector instead — so the registered s16 row's
        ;; "one template covers all" argument reaches them least of all.
        (srfi 160 c64) (srfi 160 c128))

(test-begin "homogeneous-vectors")

;; ---------------------------------------------------------------------------
;; SRFI 4: every type constructs, stores and reads back
;; ---------------------------------------------------------------------------
;; One row per type rather than a loop, so a failure names the type.

(test-equal "u8vector round-trips" '(0 1 255)
  (u8vector->list (u8vector 0 1 255)))
(test-equal "s8vector round-trips its signed ends" '(-128 0 127)
  (s8vector->list (s8vector -128 0 127)))
(test-equal "u16vector round-trips" '(0 65535)
  (u16vector->list (u16vector 0 65535)))
(test-equal "s16vector round-trips its signed ends" '(-32768 0 32767)
  (s16vector->list (s16vector -32768 0 32767)))
(test-equal "u32vector round-trips" '(0 4294967295)
  (u32vector->list (u32vector 0 4294967295)))
(test-equal "s32vector round-trips its signed ends" '(-2147483648 0 2147483647)
  (s32vector->list (s32vector -2147483648 0 2147483647)))
(test-equal "u64vector round-trips a value above 32 bits" '(0 4294967296)
  (u64vector->list (u64vector 0 4294967296)))
(test-equal "s64vector round-trips a negative below -2^31" '(-4294967296 0)
  (s64vector->list (s64vector -4294967296 0)))
(test-equal "f32vector round-trips a value it can represent exactly" '(1.5 -2.25)
  (f32vector->list (f32vector 1.5 -2.25)))
(test-equal "f64vector round-trips a value f32 could not hold" '(1.0000000000000002)
  (f64vector->list (f64vector 1.0000000000000002)))

;; The signed and unsigned types of the same width are distinct: a negative
;; value in the signed one must not read back as its unsigned complement.
(test-equal "s8vector keeps a negative negative" -1
  (s8vector-ref (s8vector -1) 0))
(test-equal "u8vector of the same width holds the complement instead" 255
  (u8vector-ref (u8vector 255) 0))

;; `make-` with and without a fill.
(test-equal "make-u8vector fills" '(7 7 7)
  (u8vector->list (make-u8vector 3 7)))
(test-equal "make-s16vector fills with a negative" '(-5 -5)
  (s16vector->list (make-s16vector 2 -5)))
(test-equal "make-f64vector has the length asked for" 4
  (f64vector-length (make-f64vector 4 0.0)))

;; `-set!` writes through, and `-ref` reads what was written.
(test-equal "u32vector-set! writes through" 70000
  (let ((v (make-u32vector 2 0)))
    (u32vector-set! v 1 70000)
    (u32vector-ref v 1)))
(test-equal "s64vector-set! writes a negative through" -70000
  (let ((v (make-s64vector 1 0)))
    (s64vector-set! v 0 -70000)
    (s64vector-ref v 0)))

;; The predicates are exclusive: a vector of one type is not another, even at
;; the same width and signedness-complement.
(test-assert "u8vector? accepts its own" (u8vector? (u8vector 1)))
(test-assert "u8vector? rejects an s8vector" (not (u8vector? (s8vector 1))))
(test-assert "f32vector? rejects an f64vector"
  (not (f32vector? (f64vector 1.0))))
(test-assert "u8vector? rejects a plain vector" (not (u8vector? (vector 1))))
;; A u8vector *is* a bytevector here — the representation is shared, and SRFI 4
;; neither requires nor forbids that. Asserted rather than left implicit,
;; because a reader coming from an implementation where the two are distinct
;; types would otherwise write code that depends on telling them apart.
(test-assert "a u8vector is a bytevector" (u8vector? (bytevector 1)))
(test-assert "a bytevector is a u8vector" (bytevector? (u8vector 1)))

;; `list->` is the inverse of `->list`.
(test-equal "list->s32vector inverts s32vector->list" '(-1 0 1)
  (s32vector->list (list->s32vector '(-1 0 1))))
(test-equal "an empty homogeneous vector round-trips" '()
  (u16vector->list (list->u16vector '())))

;; ---------------------------------------------------------------------------
;; An out-of-range store is refused, and by every type alike
;; ---------------------------------------------------------------------------
;; See the header: the value being asserted is the *uniformity*, not the
;; choice. One row per width and signedness, since each bottoms out in a
;; different accessor and the original defect was exactly that they disagreed.

(define (refuses? thunk) (guard (e (#t #t)) (thunk) #f))

(test-assert "u8vector-set! refuses 256"
  (refuses? (lambda () (u8vector-set! (make-u8vector 1 0) 0 256))))
(test-assert "s8vector-set! refuses 128"
  (refuses? (lambda () (s8vector-set! (make-s8vector 1 0) 0 128))))
(test-assert "s8vector-set! refuses -129"
  (refuses? (lambda () (s8vector-set! (make-s8vector 1 0) 0 -129))))
(test-assert "u16vector-set! refuses 65536"
  (refuses? (lambda () (u16vector-set! (make-u16vector 1 0) 0 65536))))
(test-assert "s16vector-set! refuses 32768"
  (refuses? (lambda () (s16vector-set! (make-s16vector 1 0) 0 32768))))
(test-assert "u32vector-set! refuses 2^32"
  (refuses? (lambda () (u32vector-set! (make-u32vector 1 0) 0 4294967296))))
(test-assert "s32vector-set! refuses 2^31"
  (refuses? (lambda () (s32vector-set! (make-s32vector 1 0) 0 2147483648))))
(test-assert "u64vector-set! refuses -1"
  (refuses? (lambda () (u64vector-set! (make-u64vector 1 0) 0 -1))))
(test-assert "s64vector-set! refuses 2^63"
  (refuses? (lambda ()
              (s64vector-set! (make-s64vector 1 0) 0 9223372036854775808))))
;; The float types take any real; it is rounded to the stored precision, which
;; is what f32 storage means, so there is no out-of-range value to refuse.
(test-assert "f32vector-set! accepts a real it must round"
  (not (refuses? (lambda ()
                   (f32vector-set! (make-f32vector 1 0.0) 0 1.0e300)))))
(test-assert "f64vector-set! refuses a non-real"
  (refuses? (lambda () (f64vector-set! (make-f64vector 1 0.0) 0 'nope))))
;; The ends of each range are inside it, so the check cannot be off by one.
(test-assert "the ends of each range are accepted"
  (not (refuses? (lambda ()
                   (u8vector-set! (make-u8vector 1 0) 0 255)
                   (s8vector-set! (make-s8vector 1 0) 0 -128)
                   (u64vector-set! (make-u64vector 1 0) 0 18446744073709551615)
                   (s64vector-set! (make-s64vector 1 0) 0
                                   -9223372036854775808)))))

;; ---------------------------------------------------------------------------
;; SRFI 160 base: the same twelve types, through the library that layers on it
;; ---------------------------------------------------------------------------
;; `(srfi 160 base)` re-exports SRFI 4's constructors and adds the two complex
;; types, which SRFI 4 has no equivalent of. Upstream's registered suite never
;; reaches these.

(test-equal "c64vector round-trips a complex value" 1.0+2.0i
  (c64vector-ref (c64vector 1+2i) 0))
(test-equal "c128vector round-trips a complex value" 5.0+6.0i
  (c128vector-ref (c128vector 5+6i) 0))
(test-equal "c64vector has the length asked for" 2
  (c64vector-length (c64vector 1+1i 2+2i)))
(test-assert "c64vector? rejects a c128vector"
  (not (c64vector? (c128vector 1+1i))))
(test-equal "a complex vector holds a real value too" 3.0
  (real-part (c128vector-ref (c128vector 3) 0)))

;; ---------------------------------------------------------------------------
;; SRFI 160 per-type: the operations upstream exercises for s16 only
;; ---------------------------------------------------------------------------
;; A sample across widths and signedness rather than all twelve of every
;; operation: the point is that the expansion produced a working library for
;; types other than s16, not to re-test the template.

(test-equal "u8vector-map applies to every element" '(2 4 6)
  (u8vector->list (u8vector-map (lambda (x) (* x 2)) (u8vector 1 2 3))))
(test-equal "f64vector-fold accumulates" 6.0
  (f64vector-fold + 0.0 (f64vector 1.0 2.0 3.0)))
(test-equal "s32vector-append joins two vectors" '(1 2 3 4)
  (s32vector->list (s32vector-append (s32vector 1 2) (s32vector 3 4))))
(test-assert "u64vector-any finds a match"
  (u64vector-any even? (u64vector 1 2 3)))
(test-assert "s8vector-every is false when one element fails"
  (not (s8vector-every even? (s8vector 2 3))))
(test-equal "u16vector-count counts matches" 2
  (u16vector-count even? (u16vector 1 2 3 4)))
(test-equal "f32vector->vector produces a general vector" 2
  (vector-length (f32vector->vector (f32vector 1.0 2.0))))
(test-assert "u8vector= compares by contents"
  (u8vector= (u8vector 1 2) (u8vector 1 2)))
(test-assert "u8vector= distinguishes different contents"
  (not (u8vector= (u8vector 1 2) (u8vector 1 3))))

;; A mutator and a reverse, on a type upstream does not touch.
(test-equal "u32vector-reverse! reverses in place" '(3 2 1)
  (let ((v (u32vector 1 2 3)))
    (u32vector-reverse! v)
    (u32vector->list v)))
(test-equal "s64vector-fill! overwrites every element" '(9 9)
  (let ((v (s64vector 1 2)))
    (s64vector-fill! v 9)
    (s64vector->list v)))

;; The two complex per-type libraries, which nothing else reaches: their
;; element type is the one the expansion does not share with SRFI 4, so an
;; operation that works on every integral type could still be wrong here.
(test-equal "c64vector-map applies to every element" '(2.0+4.0i)
  (c64vector->list (c64vector-map (lambda (z) (* z 2)) (c64vector 1+2i))))
(test-equal "c128vector-append joins two vectors" 3
  (c128vector-length (c128vector-append (c128vector 1+1i) (c128vector 2+2i 3+3i))))
(test-assert "c64vector= compares by contents"
  (c64vector= (c64vector 1+1i) (c64vector 1+1i)))
(test-equal "c128vector-fold accumulates complex values" 4.0+6.0i
  (c128vector-fold + 0 (c128vector 1+2i 3+4i)))
(test-equal "c64vector-reverse! reverses in place" 2.0
  (let ((v (c64vector 1+1i 2+2i)))
    (c64vector-reverse! v)
    (real-part (c64vector-ref v 0))))

;; ---------------------------------------------------------------------------
;; SRFI 160 writer and hash regressions (#384)
;; ---------------------------------------------------------------------------
;; The upstream writer read element zero of an empty vector after emitting
;; its prefix. Check complete output, not just that it returns normally.
(define (vector-written writer vec)
  (let ((port (open-output-string)))
    (writer vec port)
    (get-output-string port)))

(test-equal "write-u8vector handles an empty vector" "#u8()"
  (vector-written write-u8vector (u8vector)))
(test-equal "write-s8vector handles an empty vector" "#s8()"
  (vector-written write-s8vector (s8vector)))
(test-equal "write-u16vector handles an empty vector" "#u16()"
  (vector-written write-u16vector (u16vector)))
(test-equal "write-s16vector handles an empty vector" "#s16()"
  (vector-written write-s16vector (s16vector)))
(test-equal "write-u32vector handles an empty vector" "#u32()"
  (vector-written write-u32vector (u32vector)))
(test-equal "write-s32vector handles an empty vector" "#s32()"
  (vector-written write-s32vector (s32vector)))
(test-equal "write-u64vector handles an empty vector" "#u64()"
  (vector-written write-u64vector (u64vector)))
(test-equal "write-s64vector handles an empty vector" "#s64()"
  (vector-written write-s64vector (s64vector)))
(test-equal "write-f32vector handles an empty vector" "#f32()"
  (vector-written write-f32vector (f32vector)))
(test-equal "write-f64vector handles an empty vector" "#f64()"
  (vector-written write-f64vector (f64vector)))
(test-equal "write-c64vector handles an empty vector" "#c64()"
  (vector-written write-c64vector (c64vector)))
(test-equal "write-c128vector handles an empty vector" "#c128()"
  (vector-written write-c128vector (c128vector)))
(test-equal "write-u8vector writes a singleton without a space" "#u8(7)"
  (vector-written write-u8vector (u8vector 7)))
(test-equal "write-u8vector separates elements without a trailing space" "#u8(7 8)"
  (vector-written write-u8vector (u8vector 7 8)))
(test-equal "write-u8vector defaults to the current output port" "#u8()"
  (let ((port (open-output-string)))
    (parameterize ((current-output-port port))
      (write-u8vector (u8vector)))
    (get-output-string port)))

;; Hashes are nonnegative exact integers, and equal vectors hash alike.
;; Their numeric values are implementation-defined. The old element sum
;; became nonfinite for infinity inputs, infinity cancellation, and even
;; finite f64/c128 inputs that overflowed when added.
(define (valid-vector-hash? comparator vec)
  (let ((h ((comparator-hash-function comparator) vec)))
    (and (exact-integer? h) (>= h 0))))

(define (equal-vector-hashes? comparator left right)
  (and ((comparator-equality-predicate comparator) left right)
       (let ((hash (comparator-hash-function comparator)))
         (= (hash left) (hash right)))))

(test-assert "u8vector-comparator hashes ordinary elements"
  (valid-vector-hash? u8vector-comparator (u8vector 0 255)))
(test-assert "s8vector-comparator hashes ordinary elements"
  (valid-vector-hash? s8vector-comparator (s8vector -128 127)))
(test-assert "u16vector-comparator hashes ordinary elements"
  (valid-vector-hash? u16vector-comparator (u16vector 0 65535)))
(test-assert "s16vector-comparator hashes ordinary elements"
  (valid-vector-hash? s16vector-comparator (s16vector -32768 32767)))
(test-assert "u32vector-comparator hashes ordinary elements"
  (valid-vector-hash? u32vector-comparator (u32vector 0 4294967295)))
(test-assert "s32vector-comparator hashes ordinary elements"
  (valid-vector-hash? s32vector-comparator (s32vector -2147483648 2147483647)))
(test-assert "u64vector-comparator hashes ordinary elements"
  (valid-vector-hash? u64vector-comparator (u64vector 0 18446744073709551615)))
(test-assert "s64vector-comparator hashes ordinary elements"
  (valid-vector-hash? s64vector-comparator
                      (s64vector -9223372036854775808 9223372036854775807)))
(test-assert "f32vector-comparator hashes ordinary elements"
  (valid-vector-hash? f32vector-comparator (f32vector -1.5 2.25)))
(test-assert "f64vector-comparator hashes ordinary elements"
  (valid-vector-hash? f64vector-comparator (f64vector -1.5 2.25)))
(test-assert "c64vector-comparator hashes ordinary elements"
  (valid-vector-hash? c64vector-comparator (c64vector -1.5+2.25i)))
(test-assert "c128vector-comparator hashes ordinary elements"
  (valid-vector-hash? c128vector-comparator (c128vector -1.5+2.25i)))

(test-assert "f32vector-comparator hashes an empty vector"
  (valid-vector-hash? f32vector-comparator (f32vector)))
(test-assert "f64vector-comparator hashes an empty vector"
  (valid-vector-hash? f64vector-comparator (f64vector)))
(test-assert "c64vector-comparator hashes an empty vector"
  (valid-vector-hash? c64vector-comparator (c64vector)))
(test-assert "c128vector-comparator hashes an empty vector"
  (valid-vector-hash? c128vector-comparator (c128vector)))

(test-assert "f32vector-comparator hashes an infinity"
  (valid-vector-hash? f32vector-comparator (f32vector +inf.0)))
(test-assert "f64vector-comparator hashes an infinity"
  (valid-vector-hash? f64vector-comparator (f64vector +inf.0)))
(test-assert "f32vector-comparator hashes a negative infinity"
  (valid-vector-hash? f32vector-comparator (f32vector -inf.0)))
(test-assert "f64vector-comparator hashes a negative infinity"
  (valid-vector-hash? f64vector-comparator (f64vector -inf.0)))
(test-assert "c64vector-comparator hashes an infinite real component"
  (valid-vector-hash? c64vector-comparator
                      (c64vector (make-rectangular +inf.0 1.0))))
(test-assert "c128vector-comparator hashes an infinite real component"
  (valid-vector-hash? c128vector-comparator
                      (c128vector (make-rectangular -inf.0 1.0))))
(test-assert "c64vector-comparator hashes an infinite imaginary component"
  (valid-vector-hash? c64vector-comparator
                      (c64vector (make-rectangular 1.0 -inf.0))))
(test-assert "c128vector-comparator hashes an infinite imaginary component"
  (valid-vector-hash? c128vector-comparator
                      (c128vector (make-rectangular 1.0 +inf.0))))

(test-assert "f32vector-comparator hashes mixed infinities"
  (valid-vector-hash? f32vector-comparator (f32vector +inf.0 -inf.0)))
(test-assert "f64vector-comparator hashes mixed infinities"
  (valid-vector-hash? f64vector-comparator (f64vector +inf.0 -inf.0)))
(test-assert "c64vector-comparator hashes mixed infinities"
  (valid-vector-hash? c64vector-comparator
    (c64vector (make-rectangular +inf.0 -inf.0)
               (make-rectangular -inf.0 +inf.0))))
(test-assert "c128vector-comparator hashes mixed infinities"
  (valid-vector-hash? c128vector-comparator
    (c128vector (make-rectangular +inf.0 -inf.0)
                (make-rectangular -inf.0 +inf.0))))
(test-assert "f64vector-comparator hashes finite elements whose sum overflows"
  (valid-vector-hash? f64vector-comparator (f64vector 1e308 1e308)))
(test-assert "c128vector-comparator hashes finite elements whose sum overflows"
  (valid-vector-hash? c128vector-comparator
    (c128vector (make-rectangular 1e308 1e308)
                (make-rectangular 1e308 1e308))))

;; Numerically equal elements, including signed zeros and infinities, must
;; still produce equal hashes when the vectors are separate allocations.
(test-assert "equal f32vectors hash alike with signed zeros and infinities"
  (equal-vector-hashes? f32vector-comparator
    (f32vector 0.0 1.5 +inf.0 -inf.0)
    (f32vector -0.0 1.5 +inf.0 -inf.0)))
(test-assert "equal f64vectors hash alike with signed zeros and infinities"
  (equal-vector-hashes? f64vector-comparator
    (f64vector 0.0 1.5 +inf.0 -inf.0)
    (f64vector -0.0 1.5 +inf.0 -inf.0)))
(test-assert "equal c64vectors hash alike with signed zeros and infinities"
  (equal-vector-hashes? c64vector-comparator
    (c64vector (make-rectangular 0.0 -0.0) (make-rectangular +inf.0 -inf.0))
    (c64vector (make-rectangular -0.0 0.0) (make-rectangular +inf.0 -inf.0))))
(test-assert "equal c128vectors hash alike with signed zeros and infinities"
  (equal-vector-hashes? c128vector-comparator
    (c128vector (make-rectangular 0.0 -0.0) (make-rectangular +inf.0 -inf.0))
    (c128vector (make-rectangular -0.0 0.0) (make-rectangular +inf.0 -inf.0))))

;; SRFI 128 excludes NaNs from its comparator guarantees. Patina's existing
;; number-hash also handles them; these rows preserve that extra robustness
;; for the vector hash procedure without requiring equality/ordering of NaNs.
(test-assert "f32vector-comparator hash accepts NaN as an extension"
  (valid-vector-hash? f32vector-comparator (f32vector +nan.0)))
(test-assert "f64vector-comparator hash accepts NaN as an extension"
  (valid-vector-hash? f64vector-comparator (f64vector +nan.0)))
(test-assert "c64vector-comparator hash accepts NaN components as an extension"
  (and (valid-vector-hash? c64vector-comparator
         (c64vector (make-rectangular +nan.0 1.0)))
       (valid-vector-hash? c64vector-comparator
         (c64vector (make-rectangular 1.0 +nan.0)))))
(test-assert "c128vector-comparator hash accepts NaN components as an extension"
  (and (valid-vector-hash? c128vector-comparator
         (c128vector (make-rectangular +nan.0 1.0)))
       (valid-vector-hash? c128vector-comparator
         (c128vector (make-rectangular 1.0 +nan.0)))))

(test-end)
