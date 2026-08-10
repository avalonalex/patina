;; SRFI 151: Bitwise Operations
;;
;; R7RS-large names this (scheme bitwise) — see lib/scheme/bitwise.sld.
;;
;; The core operators are Rust primitives in (patina internal bitwise): written
;; in portable Scheme, a bitwise-and over a bignum is unusably slow, which is
;; the reason this library is bundled rather than left to the corpus. The ~30
;; derived procedures are Scheme, ported from Alex Shinn's reference
;; implementation.

(define-library (srfi 151)
  (import (scheme base)
          (patina internal bitwise))
  (export
    ;; Core (primitives)
    bitwise-not bitwise-and bitwise-ior bitwise-xor
    arithmetic-shift bit-count integer-length bit-set?
    ;; Derived logical operators
    bitwise-eqv bitwise-nand bitwise-nor
    bitwise-andc1 bitwise-andc2 bitwise-orc1 bitwise-orc2
    bitwise-if
    ;; Bit tests
    any-bit-set? every-bit-set? first-set-bit
    ;; Bit fields
    bit-field bit-field-any? bit-field-every?
    bit-field-clear bit-field-set
    bit-field-replace bit-field-replace-same
    bit-field-rotate bit-field-reverse
    copy-bit bit-swap
    ;; Conversion
    bits->list list->bits bits->vector vector->bits bits
    ;; Iteration
    bitwise-fold bitwise-for-each bitwise-unfold
    make-bitwise-generator)
  (include "151/bitwise.scm"))
