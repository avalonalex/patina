;; SRFI 143: Fixnums
;;
;; R7RS-large names this (scheme fixnum) — see lib/scheme/fixnum.sld.
;;
;; Almost entirely renames: the bitwise operators come from SRFI 151 and the
;; arithmetic from (scheme base), because on a fixnum argument those already do
;; the right thing. What the library genuinely has to get right is fx-width,
;; fx-greatest and fx-least, which must describe Patina's actual fixnum
;; representation rather than a convention — so they are derived from it.

(define-library (srfi 143)
  (import (scheme base)
          (only (patina internal bitwise) fixnum?)
          (rename (srfi 151)
                  (bitwise-not fxnot)
                  (bitwise-and fxand)
                  (bitwise-ior fxior)
                  (bitwise-xor fxxor)
                  (arithmetic-shift fxarithmetic-shift)
                  (bit-count fxbit-count)
                  (integer-length fxlength)
                  (bitwise-if fxif)
                  (bit-set? fxbit-set?)
                  (copy-bit fxcopy-bit)
                  (first-set-bit fxfirst-set-bit)
                  (bit-field fxbit-field)
                  (bit-field-rotate fxbit-field-rotate)
                  (bit-field-reverse fxbit-field-reverse)))
  (export
    fx-width fx-greatest fx-least fixnum?
    fx=? fx<? fx>? fx<=? fx>=?
    fxzero? fxpositive? fxnegative? fxodd? fxeven? fxmax fxmin
    fx+ fx- fxneg fx* fxabs fxsquare fxsqrt
    fxquotient fxremainder
    fx+/carry fx-/carry fx*/carry
    fxnot fxand fxior fxxor
    fxarithmetic-shift fxarithmetic-shift-left fxarithmetic-shift-right
    fxbit-count fxlength
    fxbit-field fxbit-field-rotate fxbit-field-reverse
    fxif fxbit-set? fxcopy-bit fxfirst-set-bit)
  (include "143/fixnum.scm"))
