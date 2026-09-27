(import (scheme base) (pfds bitwise) (patina compat smoke))
(check-equal "set and unset preserve other bits" '(11 3)
  (list (bitwise-bit-set 10 0) (bitwise-bit-unset 11 3)))
(check-equal "bit predicate distinguishes set and clear bits" '(#t #t #f)
  (list (bitwise-bit-set? 10 1) (bitwise-bit-set? 10 3)
        (bitwise-bit-set? 10 0)))
(check-equal "setting and clearing are idempotent" '(8 0)
  (list (bitwise-bit-set (bitwise-bit-set 0 3) 3)
        (bitwise-bit-unset (bitwise-bit-unset 8 3) 3)))
(define high (bitwise-bit-set 1 80))
(check-equal "high bits work beyond fixnum width" (list (+ (expt 2 80) 1) #t #t 1)
  (list high (bitwise-bit-set? high 80) (bitwise-bit-set? high 0)
        (bitwise-bit-unset high 80)))
(check-equal "arithmetic right shift preserves the sign" '(1 128 -5 -1)
  ;; Chibi 0.12's native SRFI 151 returns 0 for (-1 >> 100); Gauche and
  ;; both Patina backends retain the sign. Keep the expected arithmetic shift.
  ;; Already classified in tests/scheme/DIVERGENCES.tsv under srfi/bitwise.scm,
  ;; "a negative operand fills to -1".
  (list (bitwise-arithmetic-shift-right 128 7)
        (bitwise-arithmetic-shift-right 128 0)
        (bitwise-arithmetic-shift-right -9 1)
        (bitwise-arithmetic-shift-right -1 100)))
(define negative (bitwise-bit-unset -1 80))
(check-equal "clearing a high bit of a negative integer"
  (list (- -1 (expt 2 80)) #f #t -1)
  (list negative (bitwise-bit-set? negative 80) (bitwise-bit-set? negative 81)
        (bitwise-bit-set negative 80)))
(smoke-finish)
