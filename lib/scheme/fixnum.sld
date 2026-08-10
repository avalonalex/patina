;; (scheme fixnum) - R7RS-large Tangerine Edition
;;
;; Fixnums.
;;
;; R7RS-large names this library `(scheme fixnum)`; it is SRFI 143 under its
;; standard-track name. This is a pure re-export of `(srfi 143)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme fixnum)
  (import (srfi 143))
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
    fxif fxbit-set? fxcopy-bit fxfirst-set-bit))
