;; SRFI 143 fixnums.
;;
;; Migrated from `crates/patina-tests/tests/srfi_143_fixnum.rs` (#193), which
;; is deleted. Upstream's own suite runs in `upstream_srfi_suites.rs`.
;;
;; Almost all of the library is renames — the bitwise operators from SRFI 151,
;; the arithmetic from `(scheme base)` — because on a fixnum argument those
;; already do the right thing and SRFI 143 leaves non-fixnum arguments
;; unspecified.
;;
;; The part that can actually be wrong is `fx-width` / `fx-greatest` /
;; `fx-least`: they are claims about Patina's representation, and a library
;; that overstates the range is worse than no library. They are derived by
;; probing `fixnum?` rather than hardcoded, so the rows below check the
;; boundary rather than trusting a constant.
;;
;; **Every boundary row is written relative to `fx-width`**, except the one
;; that records today's width. The Rust original spelled 60 and 59 where this
;; says `(- fx-width 1)` and `(- fx-width 2)`, which is the same row on Patina
;; and a portable one everywhere else: chibi and Gauche have wider fixnums,
;; and the property under test is where the edge is, not which number it is.

(import (scheme base) (srfi 143) (srfi 151)
        (prefix (scheme fixnum) scheme-fixnum:)
        (srfi 64))

(test-begin "fixnums")

;; The boundary claims must match reality, whatever the tagging happens to be.
;; Asserting the invariant rather than the number means retagging Patina moves
;; these automatically instead of leaving a library that lies.
(test-assert "fx-greatest is a fixnum" (fixnum? fx-greatest))
(test-assert "one past fx-greatest is not" (not (fixnum? (+ fx-greatest 1))))
(test-assert "fx-least is a fixnum" (fixnum? fx-least))
(test-assert "one below fx-least is not" (not (fixnum? (- fx-least 1))))
;; fx-width counts the sign bit, so the range is +-2^(w-1).
(test-assert "fx-greatest is 2^(w-1) - 1"
  (= fx-greatest (- (expt 2 (- fx-width 1)) 1)))
(test-assert "fx-least is -2^(w-1)" (= fx-least (- (expt 2 (- fx-width 1)))))

;; Today's values, recorded so a change to the tagging shows up as a failing
;; row to review rather than passing silently. Scoped to Patina because the
;; numbers are Patina's representation and nobody else's.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "Patina's fixnums are 61 bits wide"
  '(61 1152921504606846975 -1152921504606846976)
  (list fx-width fx-greatest fx-least))

(test-equal "fx+" 5 (fx+ 2 3))
(test-equal "fx-" 7 (fx- 10 3))
(test-equal "fx*" 42 (fx* 6 7))
(test-equal "fxneg" -5 (fxneg 5))
(test-equal "fxabs" 5 (fxabs -5))
(test-equal "fxsquare" 25 (fxsquare 5))
(test-equal "fxquotient" 3 (fxquotient 17 5))
(test-equal "fxremainder" 2 (fxremainder 17 5))
(test-equal "sign predicates" '(#t #t #t)
  (list (fxzero? 0) (fxpositive? 1) (fxnegative? -1)))
(test-equal "parity predicates" '(#t #t) (list (fxodd? 3) (fxeven? 4)))
(test-equal "fxmax and fxmin" '(5 1) (list (fxmax 1 5 3) (fxmin 1 5 3)))
(test-equal "comparisons" '(#t #t #t) (list (fx=? 1 1) (fx<? 1 2) (fx>=? 2 2)))

;; The bitwise renames reach SRFI 151.
(test-equal "fxand" 8 (fxand 12 10))
(test-equal "fxior" 14 (fxior 12 10))
(test-equal "fxxor" 6 (fxxor 12 10))
(test-equal "fxnot" -13 (fxnot 12))
(test-equal "fxbit-count" 2 (fxbit-count 12))
(test-equal "fxlength" 4 (fxlength 12))
(test-assert "fxbit-set?" (fxbit-set? 2 12))
(test-equal "fxfirst-set-bit" 3 (fxfirst-set-bit 8))
(test-equal "fxbit-field" 10 (fxbit-field #b1101101010 0 4))
(test-equal "fxif" 9 (fxif 3 1 8))

;; SRFI 143 has both a signed shift and directional ones; the right shift is
;; the easy one to get backwards.
(test-equal "fxarithmetic-shift left" 1024 (fxarithmetic-shift 1 10))
(test-equal "fxarithmetic-shift right" 64 (fxarithmetic-shift 1024 -4))
(test-equal "fxarithmetic-shift-left" 1024 (fxarithmetic-shift-left 1 10))
(test-equal "fxarithmetic-shift-right" 64 (fxarithmetic-shift-right 1024 4))
;; Right shift is arithmetic, so the sign propagates.
(test-equal "fxarithmetic-shift-right keeps the sign" -2
  (fxarithmetic-shift-right -8 2))

;; The carry operators split a result too wide for a fixnum into a fixnum plus
;; a carry, using balanced division so the remainder lands in
;; [-2^w/2, 2^w/2).
(test-equal "fx+/carry" '(6 0)
  (call-with-values (lambda () (fx+/carry 1 2 3)) list))
(test-equal "fx-/carry" '(5 0)
  (call-with-values (lambda () (fx-/carry 10 3 2)) list))
(test-equal "fx*/carry" '(42 0)
  (call-with-values (lambda () (fx*/carry 6 7 0)) list))
;; A sum that overflows the fixnum range carries out rather than silently
;; producing a bignum.
(test-equal "an overflowing sum carries out" '(#t 1)
  (call-with-values (lambda () (fx+/carry fx-greatest fx-greatest 0))
    (lambda (r c) (list (fixnum? r) c))))

;; The exact half-point: a wide result of precisely 2^(fx-width-1) must land on
;; fx-least with a carry of 1. A round-based balanced division puts it on the
;; excluded endpoint +2^(fx-width-1) instead (round is half-to-even), returning
;; a non-fixnum from the operators that exist to handle this boundary.
(test-equal "fx+/carry at the half-point lands on fx-least" '(#t #t 1)
  (call-with-values (lambda () (fx+/carry fx-greatest 1 0))
    (lambda (r c) (list (fixnum? r) (eqv? r fx-least) c))))
(test-equal "fx-/carry at the half-point lands on fx-greatest" '(#t #t -1)
  (call-with-values (lambda () (fx-/carry fx-least 1 0))
    (lambda (r c) (list (fixnum? r) (eqv? r fx-greatest) c))))
;; The multiplicative half-point: fx-least * 1 + 0 stays put with no carry.
(test-equal "fx*/carry leaves fx-least alone" '(#t 0)
  (call-with-values (lambda () (fx*/carry fx-least 1 0))
    (lambda (r c) (list (eqv? r fx-least) c))))

(test-equal "fxsqrt returns the root and the remainder" '(4 1)
  (call-with-values (lambda () (fxsqrt 17)) list))

;; R7RS-large names this `(scheme fixnum)`. Imported with a prefix so the row
;; does not depend on whether an implementation's two names share bindings —
;; the next row is the one about that.
(test-equal "(scheme fixnum) is the same library" '(5 8 #t)
  (list (scheme-fixnum:fx+ 2 3)
        (scheme-fixnum:fxand 12 10)
        (= scheme-fixnum:fx-width fx-width)))

;; The fx bitwise operators agree with SRFI 151's, which they rename.
(test-equal "fxand agrees with bitwise-and" '(8 8)
  (list (fxand 12 10) (bitwise-and 12 10)))

;; The fx ops at the representation's own edges — the fast-path seam. This
;; file's method is to check the boundary rather than trust a constant; these
;; extend that to the bitwise aliases, whose shared primitives take an i64
;; fast path exactly when every operand is a fixnum.
(test-equal "fxand at fx-least" fx-least (fxand fx-least -1))
(test-equal "fxnot of fx-least" fx-greatest (fxnot fx-least))
(test-equal "fxnot of fx-greatest" fx-least (fxnot fx-greatest))
(test-equal "fxxor of the two extremes" -1 (fxxor fx-greatest fx-least))
(test-equal "the widest left shift that stays a fixnum"
  (expt 2 (- fx-width 2))
  (fxarithmetic-shift 1 (- fx-width 2)))
(test-equal "fx-least shifted right to the sign bit" -1
  (fxarithmetic-shift fx-least (- 1 fx-width)))
(test-equal "fxbit-count of fx-greatest" (- fx-width 1) (fxbit-count fx-greatest))
(test-equal "fxlength of fx-greatest" (- fx-width 1) (fxlength fx-greatest))
(test-assert "the sign bit of -1 is set" (fxbit-set? (- fx-width 1) -1))
(test-assert "the sign bit of fx-greatest is clear"
  (not (fxbit-set? (- fx-width 1) fx-greatest)))

(test-end)
