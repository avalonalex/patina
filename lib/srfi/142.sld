;; SRFI 142: Bitwise Operations (withdrawn)
;;
;; Withdrawn in favour of SRFI 151, but packages written against it still
;; import it by name (jkode-sassy in the vendored corpus), so it is a rename
;; over the same primitives rather than a second implementation — the same
;; treatment as (srfi 33) and (srfi 60).
;;
;; Two deviations from a plain re-export, both from the SRFI texts:
;; - 142's `bitwise-if` takes bits from its *third* argument where the mask
;;   bit is 1, the opposite of 151's, so the trailing arguments are swapped
;;   rather than aliased. Same resolution as chibi's own (srfi 142) shim.
;; - 142 spells the LSB-first list conversions `integer->list` /
;;   `list->integer`; 151 renamed them `bits->list` / `list->bits` without
;;   changing the bit order, so those really are plain renames — unlike
;;   SRFI 60's MSB-first `integer->list`, which 60.sld must reverse.

(define-library (srfi 142)
  (import (scheme base)
          (rename (srfi 151)
                  (bitwise-if srfi-151:bitwise-if)
                  (bits->list integer->list)
                  (list->bits list->integer)
                  (bits->vector integer->vector)
                  (vector->bits vector->integer)))
  (export bitwise-not
          bitwise-and bitwise-ior
          bitwise-xor bitwise-eqv
          bitwise-nand bitwise-nor
          bitwise-andc1 bitwise-andc2
          bitwise-orc1 bitwise-orc2
          arithmetic-shift bit-count integer-length
          bitwise-if
          bit-set? any-bit-set? every-bit-set?
          first-set-bit
          bit-field bit-field-any? bit-field-every?
          bit-field-clear bit-field-set
          bit-field-replace bit-field-replace-same
          bit-field-rotate bit-field-reverse
          copy-bit integer->list list->integer
          integer->vector vector->integer
          bits bit-swap
          bitwise-fold bitwise-for-each bitwise-unfold
          make-bitwise-generator)
  (begin
    (define (bitwise-if mask n m)
      (srfi-151:bitwise-if mask m n))))
