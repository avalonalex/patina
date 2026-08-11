;; SRFI 60: Integer Bitwise Operations (Aubrey Jaffer)
;;
;; The largest measured gap in the vendored corpus — 31 packages import it,
;; more than any other library Patina did not provide. Superseded by SRFI 151,
;; but the names differ (logand vs bitwise-and, ash vs arithmetic-shift), and
;; SRFI 60 exports *both* spellings for eight of them, so importing 151 is not a
;; substitute.
;;
;; Deliberately a single plain import with the alternate spellings defined
;; below, rather than `(only ...)` plus `(rename ...)` over the same library.
;; That shape is valid R7RS and works on the VM, but the tree-walker drops the
;; rename set's pass-through names — see the note in
;; PRD/TRACK_L_SNOW_LIBRARIES_PRD.md.

(define-library (srfi 60)
  (import (scheme base) (srfi 151))
  (export logand bitwise-and logior bitwise-ior logxor bitwise-xor
          lognot bitwise-not bitwise-if bitwise-merge
          logtest any-bits-set? logcount bit-count integer-length
          log2-binary-factors first-set-bit
          logbit? bit-set? copy-bit bit-field copy-bit-field
          ash arithmetic-shift rotate-bit-field reverse-bit-field
          integer->list list->integer booleans->integer)
  (begin
    ;; SRFI 60's spellings for operators SRFI 151 names differently.
    (define logand bitwise-and)
    (define logior bitwise-ior)
    (define logxor bitwise-xor)
    (define lognot bitwise-not)
    (define bitwise-merge bitwise-if)
    (define logtest any-bit-set?)
    (define any-bits-set? any-bit-set?)
    (define logcount bit-count)
    (define logbit? bit-set?)
    (define ash arithmetic-shift)
    (define rotate-bit-field bit-field-rotate)
    (define reverse-bit-field bit-field-reverse)
    (define log2-binary-factors first-set-bit)

    ;; SRFI 60's list conversions are MSB-first; SRFI 151's bits->list family
    ;; is LSB-first, and 151 renamed them precisely because the order differs.
    ;; Aliasing instead of reversing silently flips every bit pattern.
    (define (integer->list k . len)
      (reverse (apply bits->list k len)))
    (define (list->integer bools)
      (list->bits (reverse bools)))
    (define (booleans->integer . bools)
      (list->integer bools))

    ;; One more that SRFI 60 adds.
    (define (copy-bit-field to from start end)
      (bitwise-if (arithmetic-shift (- (arithmetic-shift 1 (- end start)) 1) start)
                  (arithmetic-shift from start)
                  to))))
