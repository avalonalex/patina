;; Which numbers are integers, rationals, reals and complexes — R7RS §6.2.6 —
;; and the integer operations that follow from the answer.
;;
;; Migrated from the numeric half of
;; `crates/patina-tests/tests/compliance/predicates.rs` (#193), which is
;; deleted; the rest went to `predicates.scm`. One row here per Rust
;; assertion, less three the Rust file made twice: `(real? 3+4i)`,
;; `(integer? 3+4i)` and `(rational? 3+4i)` were in both its predicate tests
;; and its zero-imaginary-part test, with the same answers, and are rows once.
;;
;; The subject is the numeric tower's predicates, and in particular the two
;; places R7RS is less obvious than it looks: an inexact number can be an
;; integer (`(integer? 3.0)`), and every finite inexact is rational. Once
;; `integer?` says yes, the integer operations — quotient, remainder, modulo,
;; gcd, lcm, numerator, denominator — must accept the number and answer
;; inexactly, which is the second half of the file.

(import (scheme base) (srfi 64))

(test-begin "numeric-predicates")

;; ─── The tower's predicates ──────────────────────────────────────────────────

(test-equal "complex? of a complex" #t (complex? 3+4i))
(test-equal "complex? of an imaginary number" #t (complex? 0+1i))
(test-equal "complex? of a real" #t (complex? 3.14))
(test-equal "complex? of a ratio" #t (complex? 22/7))
(test-equal "complex? of an integer" #t (complex? 42))
(test-equal "complex? of a string" #f (complex? "42"))
(test-equal "complex? of a boolean" #f (complex? #t))

(test-equal "real? of an inexact" #t (real? 3.14))
(test-equal "real? of a ratio" #t (real? 22/7))
(test-equal "real? of an integer" #t (real? 42))
(test-equal "real? of a negative integer" #t (real? -5))
(test-equal "real? of a complex" #f (real? 3+4i))
(test-equal "real? of a string" #f (real? "3.14"))
(test-equal "real? of a boolean" #f (real? #f))

(test-equal "rational? of 22/7" #t (rational? 22/7))
(test-equal "rational? of 3/4" #t (rational? 3/4))
(test-equal "rational? of an integer" #t (rational? 42))
(test-equal "rational? of a negative integer" #t (rational? -5))
;; Every finite inexact number is a ratio of two integers.
(test-equal "rational? of a finite inexact" #t (rational? 3.14))
(test-equal "rational? of a complex" #f (rational? 3+4i))
(test-equal "rational? of a string" #f (rational? "22/7"))

(test-equal "integer? of an integer" #t (integer? 42))
(test-equal "integer? of a negative integer" #t (integer? -5))
(test-equal "integer? of zero" #t (integer? 0))
(test-equal "integer? of 22/7" #f (integer? 22/7))
(test-equal "integer? of 3/4" #f (integer? 3/4))
(test-equal "integer? of 3.14" #f (integer? 3.14))
(test-equal "integer? of the inexact 3.0" #t (integer? 3.0))
(test-equal "integer? of a complex" #f (integer? 3+4i))
(test-equal "integer? of a string" #f (integer? "42"))

(test-equal "number? of an integer" #t (number? 42))
(test-equal "number? of a ratio" #t (number? 22/7))
(test-equal "number? of an inexact" #t (number? 3.14))
(test-equal "number? of a complex" #t (number? 3+4i))
(test-equal "number? of a string" #f (number? "42"))
(test-equal "number? of a boolean" #f (number? #t))
(test-equal "number? of a symbol" #f (number? 'symbol))

;; ─── Inexact integers ────────────────────────────────────────────────────────

(test-equal "integer? of 4.0" #t (integer? 4.0))
(test-equal "integer? of -0.0" #t (integer? -0.0))
(test-equal "integer? of 1000000.0" #t (integer? 1000000.0))
(test-equal "integer? of 4.5" #f (integer? 4.5))
(test-equal "integer? of 0.1" #f (integer? 0.1))
(test-equal "rational? of 4.0" #t (rational? 4.0))
(test-equal "rational? of 3.14159" #t (rational? 3.14159))
(test-equal "rational? of 0.1" #t (rational? 0.1))

;; A complex number whose imaginary part is an exact zero is a real, and is an
;; integer or a rational exactly when its real part is. The 3+4i controls are
;; the rows above.
(test-equal "real? of 3+0i" #t (real? 3+0i))
(test-equal "real? of -2.5+0i" #t (real? -2.5+0i))
(test-equal "integer? of 3+0i" #t (integer? 3+0i))
(test-equal "integer? of 4.0+0i" #t (integer? 4.0+0i))
(test-equal "integer? of 3.5+0i" #f (integer? 3.5+0i))
(test-equal "rational? of 3+0i" #t (rational? 3+0i))
(test-equal "rational? of 3.14+0i" #t (rational? 3.14+0i))

;; ─── The integer operations ──────────────────────────────────────────────────

;; modulo's result takes the divisor's sign. That is what separates it from
;; remainder, and from a Euclidean remainder, which is never negative.
(test-equal "modulo 13 4" 1 (modulo 13 4))
(test-equal "modulo 13 -4" -3 (modulo 13 -4))
(test-equal "modulo -13 4" 3 (modulo -13 4))
(test-equal "modulo -13 -4" -1 (modulo -13 -4))

(test-equal "odd? -1" #t (odd? -1))
(test-equal "odd? -3" #t (odd? -3))
(test-equal "odd? -2" #f (odd? -2))
(test-equal "even? -2" #t (even? -2))
(test-equal "even? -4" #t (even? -4))
(test-equal "even? -1" #f (even? -1))

;; They take inexact integers, and answer inexactly.
(test-equal "quotient of an exact by an inexact" 3.0 (quotient 13 4.0))
(test-equal "quotient of an inexact by an exact" 3.0 (quotient 13.0 4))
(test-equal "quotient of two inexacts" 3.0 (quotient 13.0 4.0))
(test-equal "remainder of an exact by an inexact" -1.0 (remainder -13 -4.0))
(test-equal "remainder of an inexact by an exact" -1.0 (remainder -13.0 -4))
(test-equal "remainder of two inexacts" -1.0 (remainder -13.0 -4.0))
(test-equal "modulo -13 4.0" 3.0 (modulo -13 4.0))
(test-equal "modulo -13.0 4" 3.0 (modulo -13.0 4))
(test-equal "modulo -13.0 4.0" 3.0 (modulo -13.0 4.0))
(test-equal "modulo -13 -4.0" -1.0 (modulo -13 -4.0))
(test-equal "modulo -13.0 -4" -1.0 (modulo -13.0 -4))
(test-equal "modulo -13.0 -4.0" -1.0 (modulo -13.0 -4.0))
(test-equal "modulo 13.0 -4" -3.0 (modulo 13.0 -4))
(test-equal "modulo 13 -4.0" -3.0 (modulo 13 -4.0))

(test-equal "gcd of an inexact and an exact" 4.0 (gcd 32.0 -36))
(test-equal "gcd of an exact and an inexact" 4.0 (gcd 32 -36.0))
(test-equal "gcd of two inexacts" 4.0 (gcd 32.0 -36.0))
(test-equal "lcm of an inexact and an exact" 288.0 (lcm 32.0 -36))
(test-equal "lcm of an exact and an inexact" 288.0 (lcm 32 -36.0))
(test-equal "lcm of two inexacts" 288.0 (lcm 32.0 -36.0))

(test-equal "numerator of 5.0" 5.0 (numerator 5.0))
(test-equal "denominator of 5.0" 1.0 (denominator 5.0))
;; 5.5 is 11/2.
(test-equal "numerator of 5.5" 11.0 (numerator 5.5))
(test-equal "denominator of 5.5" 2.0 (denominator 5.5))
(test-equal "numerator over denominator of 3.4 is 3.4" 3.4
  (/ (numerator 3.4) (denominator 3.4)))
(test-equal "numerator over denominator of 0.5 is 0.5" 0.5
  (/ (numerator 0.5) (denominator 0.5)))

;; expt answers inexactly when either operand is inexact, including at 0^0.
(test-equal "expt 0 0" 1 (expt 0 0))
(test-equal "expt 0.0 0" 1.0 (expt 0.0 0))
(test-equal "expt 0 0.0" 1.0 (expt 0 0.0))
(test-equal "expt 0.0 0.0" 1.0 (expt 0.0 0.0))

(test-end)
