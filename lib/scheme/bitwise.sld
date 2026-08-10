;; (scheme bitwise) - R7RS-large Tangerine Edition
;;
;; Bitwise operations.
;;
;; R7RS-large names this library `(scheme bitwise)`; it is SRFI 151 under its
;; standard-track name. This is a pure re-export of `(srfi 151)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme bitwise)
  (import (srfi 151))
  (export
    bitwise-not bitwise-and bitwise-ior bitwise-xor
    arithmetic-shift bit-count integer-length bit-set?
    bitwise-eqv bitwise-nand bitwise-nor
    bitwise-andc1 bitwise-andc2 bitwise-orc1 bitwise-orc2
    bitwise-if
    any-bit-set? every-bit-set? first-set-bit
    bit-field bit-field-any? bit-field-every?
    bit-field-clear bit-field-set
    bit-field-replace bit-field-replace-same
    bit-field-rotate bit-field-reverse
    copy-bit bit-swap
    bits->list list->bits bits->vector vector->bits bits
    bitwise-fold bitwise-for-each bitwise-unfold
    make-bitwise-generator))
