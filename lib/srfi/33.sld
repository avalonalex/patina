;; SRFI 33: Integer Bitwise Operations (withdrawn)
;;
;; Withdrawn in favour of SRFI 151, but 19 packages in the vendored corpus still
;; import it, so it is bundled as a rename over the same primitives rather than
;; a second implementation.

(define-library (srfi 33)
  (import (scheme base)
          (rename (srfi 151)
                  (bitwise-if bitwise-merge)
                  (any-bit-set? any-bits-set?)
                  (every-bit-set? all-bits-set?)
                  (bit-field-any? test-bit-field?)
                  (bit-field-clear clear-bit-field)))
  (export bitwise-not bitwise-and bitwise-ior bitwise-xor bitwise-eqv
          bitwise-nand bitwise-nor bitwise-andc1 bitwise-andc2
          bitwise-orc1 bitwise-orc2
          arithmetic-shift bit-count integer-length
          bitwise-merge bit-set? any-bits-set? all-bits-set? first-set-bit
          extract-bit-field test-bit-field? clear-bit-field
          replace-bit-field copy-bit-field)
  (begin
    (define (mask len)
      (- (arithmetic-shift 1 len) 1))
    (define (extract-bit-field size position n)
      (bitwise-and (arithmetic-shift n (- position)) (mask size)))
    (define (replace-bit-field size position newfield n)
      (bitwise-ior
       (bitwise-and n (bitwise-not (arithmetic-shift (mask size) position)))
       (arithmetic-shift newfield position)))
    (define (copy-bit-field to from start end)
      (bitwise-merge (arithmetic-shift (mask (- end start)) start)
                     (arithmetic-shift from start)
                     to))))
