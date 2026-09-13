;; Bytevectors — R7RS §6.9, and specifically the boundary of a byte argument.
;;
;; This file is about one question: which integers may be written where R7RS
;; says "byte". §6.9 defines a byte as an exact integer in 0..255, so the
;; answer looks settled — but `make-bytevector`'s *fill* is the place every
;; implementation is looser, because R6RS (Standard Libraries §2.1) requires
;; that argument to accept -128..255 and to store a negative one as
;; `fill + 256`. Larceny's suites assert that behaviour in both lanes, and
;; chibi and Gauche both provide it.
;;
;; So the rows below draw the line Patina now draws: the signed spelling is
;; accepted for the fill and nowhere else, and 255 is the ceiling either way.
;; The asymmetry is deliberate rather than an oversight — `bytevector` and
;; `bytevector-u8-set!` reject a negative byte in chibi and in Gauche too, and
;; matching one oracle at one site by leaving the other two is not a trade
;; worth making. The out-of-range rows are what keep the ceiling pinned; chibi
;; truncates instead of rejecting there, which `DIVERGENCES.tsv` records.
;;
;; It is not a general bytevector suite. There was no bytevector subject file
;; at all before this one, and the honest scope of what it was written for is
;; the byte boundary; anything else here is the context that makes those rows
;; readable.

(import (scheme base) (srfi 64))

(test-begin "bytevectors")

;; ─── The shape of the thing ──────────────────────────────────────────────────

(test-equal "bytevector? of a bytevector" #t (bytevector? (bytevector)))
(test-equal "bytevector? of a vector" #f (bytevector? (vector)))
(test-equal "bytevector-length of the empty bytevector" 0
  (bytevector-length (bytevector)))
(test-equal "bytevector-length of make-bytevector" 4
  (bytevector-length (make-bytevector 4 0)))

;; ─── The fill argument, unsigned ─────────────────────────────────────────────

(test-equal "an omitted fill" 0 (bytevector-u8-ref (make-bytevector 3) 0))
(test-equal "a fill of zero" 0 (bytevector-u8-ref (make-bytevector 10 0) 9))
(test-equal "a fill at the ceiling" 255
  (bytevector-u8-ref (make-bytevector 10 255) 9))

;; ─── The fill argument, signed ───────────────────────────────────────────────
;;
;; The byte a negative fill denotes is its two's complement, so -1 fills with
;; 255 and -128 with 128. Both oracles answer these the same way.

(test-equal "a fill of -1 is the byte 255" 255
  (bytevector-u8-ref (make-bytevector 10 -1) 9))
(test-equal "a fill of -128 is the byte 128" 128
  (bytevector-u8-ref (make-bytevector 10 -128) 9))
(test-equal "a signed fill fills every byte" '(255 255 255)
  (let ((b (make-bytevector 3 -1)))
    (list (bytevector-u8-ref b 0)
          (bytevector-u8-ref b 1)
          (bytevector-u8-ref b 2))))

;; ─── The ceiling holds in both directions ────────────────────────────────────
;;
;; -129 and 256 are outside R6RS's range as well as R7RS's, and Gauche rejects
;; both. chibi truncates them modulo 256 instead, which is the divergence the
;; register carries: widening to match it would accept every integer as a byte.

(test-error "a fill below -128" #t (make-bytevector 10 -129))
(test-error "a fill above 255" #t (make-bytevector 10 256))

;; ─── The signed spelling stops at the fill ───────────────────────────────────
;;
;; These are the rows that make the leniency a line rather than a slope. Both
;; oracles reject a negative byte at both sites.

(test-error "a negative byte in bytevector" #t (bytevector 1 -1 2))
(test-error "a negative byte in bytevector-u8-set!" #t
  (let ((b (make-bytevector 2 0)))
    (bytevector-u8-set! b 0 -1)))
(test-error "a byte above 255 in bytevector-u8-set!" #t
  (let ((b (make-bytevector 2 0)))
    (bytevector-u8-set! b 0 256)))

;; ─── A fill is still an integer ──────────────────────────────────────────────

(test-error "a fill that is not a number" #t (make-bytevector 2 'a))
(test-error "a fill that is inexact" #t (make-bytevector 2 1.5))

(test-end "bytevectors")
