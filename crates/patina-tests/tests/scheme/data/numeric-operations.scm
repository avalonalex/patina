;; R7RS numeric operations — §6.2.6's procedures, and what they print.
;;
;; Migrated whole from `crates/patina-tests/tests/numeric_operations.rs`
;; (#193 Phase 1). 39 `#[test]` functions over 145 assertions there; **38 rows
;; here** (37 of them, plus Larceny family 8's `rationalize` row,
;; moved in from `larceny_families.rs`), roughly one per operation, each carrying that test's cases as a list
;; so the expected value names which case drifted. Fewer rows than tests
;; because `sin`, `cos` and `tan` had a one-case test each and share a row
;; here, and because the six substring tests collapse into value assertions.
;;
;; **Extended from `compliance/numbers.rs` and `compliance/numeric_edge_cases.rs`**
;; (#193), both deleted: 53 tests over 186 assertions, which are the sections
;; from "Arithmetic" to "Integer algorithms" and the one on complex equality,
;; printing and `angle`. Their exact rationals went to `data/rationals.scm`
;; with `compliance/rationals.rs`. Two tail-call rows at depth — a countdown
;; from 100 000 and mutual `even?`/`odd?` recursion to 10 000 — are not
;; numeric, and moved to `control/tail-recursion.scm`.
;;
;; ── Two things the migration fixes, not just relocates ──────────────────────
;;
;; **These assertions had never run on the VM.** The `.rs` file carried its own
;; `assert_eval_to` built on `TreeWalkInterpreter::new_tree_walker()`, so all
;; 145 ran on the tree-walker alone — and the VM has been the default backend
;; since Phase 2. The driver runs every file on both, so the migration doubles
;; their coverage. Measured before writing this file: the VM agrees with the
;; tree-walker on all 143 of them, so nothing here was hiding a
;; divergence — but nothing was looking, either.
;;
;; **Seven tests asserted on substrings of the printed form** — `result
;; .contains("1.17")`, `contains("+")`, `contains("i")` — because a Rust string
;; comparison was the only tool to hand for a value that is only approximately
;; known. Those are now tolerance assertions on the *value*
;; (`(< (abs (- x expected)) 1e-10)`), which is both stronger and portable:
;; `contains("+")` passes on any complex number at all, and on `"+inf.0"`.
;;
;; ── Why `written` and not bare `test-equal` ────────────────────────────────
;;
;; The `.rs` helper compared `display_tagged`, which is `format_write_tagged` —
;; the datum writer. Several rows are *about* the printed form and would say
;; much less as value comparisons: `-0.0` against `0.0`, `1/10` against `0.1`,
;; the elided real part of `+i`. So the rows keep the writer under test, with
;; the helper `docs/TEST_ORGANIZATION.md` records.
;;
;; ── Measured 2026-09-07 (chibi 0.12, Gauche via `gosh -r7`) ────────────────
;;
;; Recorded in `DIVERGENCES.tsv`, not here, now that the register exists and
;; `scripts/run_suite_oracles.sh` checks it. Run that rather than trusting a
;; comment: it is the file's tallies that went stale twice during Phase 1.

(import (scheme base) (scheme write) (scheme inexact) (scheme complex) (srfi 64))

(define (written x) (let ((p (open-output-string))) (write x p) (get-output-string p)))

;; A tolerance check, for the values that are only approximately known. Note
;; before copying this one elsewhere: `magnitude` puts it in `(scheme complex)`,
;; so a file that imports only `(scheme base)` and `(scheme inexact)` gets an
;; undefined variable — and on Patina not even that, until it reaches an oracle
;; (issue #211). `1e-10`
;; is far tighter than the two or three digits the `.rs` substring assertions
;; managed, and unlike them it cannot be satisfied by an unrelated number that
;; happens to contain the digits.
;;
;; `magnitude`, not `abs`: R7RS defines `abs` on reals only, and one caller
;; passes a whole complex number — chibi's `(make-polar 1 0)` is `1.0+0.0i`,
;; where ours collapses to a real. The first draft used `abs` and chibi failed
;; that row for the helper's reason rather than the implementation's.
(define (close? x y) (< (magnitude (- x y)) 1e-10))

(test-begin "numeric-operations")

;; ── Arithmetic ─────────────────────────────────────────────────────────────
;;
;; This section down to "Rounding", and the complex rows marked below, came
;; from `compliance/numbers.rs` and `compliance/numeric_edge_cases.rs`
;; (#193), both deleted. Those helpers compared the written form too. Most
;; results here are exact integers or booleans, which print only one way, so
;; a value comparison is the same test; a ratio or a float whose spelling is
;; the point is written.

(test-equal "+, including no arguments" '(7 6 0) (list (+ 3 4) (+ 1 2 3) (+)))
(test-equal "-, including negation" '(7 5 -5) (list (- 10 3) (- 10 3 2) (- 5)))
(test-equal "*, including no arguments" '(6 24 1) (list (* 2 3) (* 2 3 4) (*)))
(test-equal "/ of exact integers is exact, and a ratio when it must be" '("5" "5/2")
  (map written (list (/ 20 4) (/ 20 4 2))))

;; An exact zero divisor is an error. An inexact one gives an infinity.
(test-error "exact division by zero is an error" #t (/ 1 0))
(test-error "exact division of a negative by zero is an error" #t (/ -5 0))
(test-equal "an inexact zero divisor gives an infinity" '(+inf.0 -inf.0)
  (list (/ 1 0.0) (/ -1 0.0)))
;; R7RS §6.2.6: "It is an error if any argument of / other than the first is
;; an exact zero." So an inexact dividend over an exact zero is Patina's
;; choice, and Gauche's; chibi raises (registered as latitude).
(test-equal "an inexact dividend over an exact zero gives an infinity" +inf.0
  (/ 1.0 0))

;; A zero divisor is *signed*, and IEEE 754 §6.3 makes the quotient's sign the
;; exclusive-or of the two. `-0.0` is not less than zero, so taking the
;; numerator's sign alone answered `+inf.0` for all four of these.
(test-equal "the sign of a zero divisor counts" '(-inf.0 +inf.0 -inf.0 -inf.0)
  (list (/ 1.0 -0.0) (/ -1.0 -0.0) (/ 1 -0.0) (/ -0.0)))

;; ── Comparisons ────────────────────────────────────────────────────────────

(test-equal "=" '(#t #f #t #f) (list (= 5 5) (= 5 6) (= 5 5 5) (= 5 5 6)))
(test-equal "<" '(#t #f #t #f) (list (< 1 2) (< 2 1) (< 1 2 3) (< 1 2 2)))
(test-equal ">" '(#t #f #t) (list (> 2 1) (> 1 2) (> 3 2 1)))
(test-equal "<=" '(#t #t #f) (list (<= 1 2) (<= 2 2) (<= 2 1)))
(test-equal ">=" '(#t #t #f) (list (>= 2 1) (>= 2 2) (>= 1 2)))

;; Every comparison involving a NaN is false, including with itself.
(test-equal "every comparison with a NaN is false" '(#f #f #f #f #f #f)
  (list (< +nan.0 0) (> +nan.0 0) (<= +nan.0 0) (>= +nan.0 0)
        (= +nan.0 0) (= +nan.0 +nan.0)))

(test-equal "an infinity is equal to itself and beyond every finite number"
  '(#f #f #t #t #t #t #t)
  (list (< +inf.0 +inf.0) (> +inf.0 +inf.0) (= +inf.0 +inf.0)
        (< 100 +inf.0) (> +inf.0 100) (< -inf.0 0) (> 0 -inf.0)))

;; ── Integer division, abs, max and min ─────────────────────────────────────

(test-equal "quotient truncates toward zero" '(3 -3)
  (list (quotient 10 3) (quotient -10 3)))
(test-equal "remainder takes the sign of the dividend" '(1 -1)
  (list (remainder 10 3) (remainder -10 3)))
(test-equal "modulo takes the sign of the divisor" '(1 2)
  (list (modulo 10 3) (modulo -10 3)))
(test-equal "abs" '(5 5 0) (list (abs 5) (abs -5) (abs 0)))
(test-equal "max and min of exact integers" '(3 3 1 1)
  (list (max 1 2 3) (max 3 2 1) (min 1 2 3) (min 3 2 1)))

;; R7RS §6.2.6: if any argument is inexact, so is the result — even when the
;; winner is the exact one.
(test-equal "max is inexact when any argument is" '(4.0 4.0 5.0)
  (list (max 3.9 4) (max 4 3.9) (max 5 3.9 4)))
(test-equal "min is inexact when any argument is" '(3.9 3.9)
  (list (min 3.9 4) (min 4 3.9)))
(test-equal "max against an infinity" '(+inf.0 +inf.0 0.0)
  (list (max 100 +inf.0) (max +inf.0 100) (max -inf.0 0)))
(test-equal "min against an infinity" '(100.0 -inf.0)
  (list (min 100 +inf.0) (min -inf.0 0)))

;; A NaN anywhere makes the result a NaN, as Chez does. R7RS says nothing
;; about NaN here; Gauche agrees, and chibi returns the other argument unless
;; the NaN comes first (registered as spec-silent). `nan?` because `equal?`
;; on two NaNs is not something R7RS pins down.
(test-equal "max with a NaN argument is a NaN" '(#t #t #t #t #t)
  (map nan? (list (max +nan.0 5) (max 1 +nan.0 3) (max 5 +nan.0)
                  (max 1 2 3 +nan.0) (max +nan.0))))
(test-equal "min with a NaN argument is a NaN" '(#t #t #t #t #t)
  (map nan? (list (min +nan.0 5) (min 1 +nan.0 3) (min 5 +nan.0)
                  (min 1 2 3 +nan.0) (min +nan.0))))

;; ── Predicates ─────────────────────────────────────────────────────────────

(test-equal "number? and integer?" '(#t #f #t #f)
  (list (number? 42) (number? 'a) (integer? 42) (integer? 3.14)))
(test-equal "zero?, positive? and negative?" '(#t #f #t #f #f #t #f #f)
  (list (zero? 0) (zero? 1) (positive? 5) (positive? -5) (positive? 0)
        (negative? -5) (negative? 5) (negative? 0)))
(test-equal "odd? and even?" '(#t #f #t #f)
  (list (odd? 3) (odd? 4) (even? 4) (even? 3)))

;; ── Exactness and contagion ────────────────────────────────────────────────

(test-equal "exact integers are exact, bignums included" '(#t #t #t #t #t #t)
  (list (exact? 42) (exact? -17) (exact? 0) (exact? 9223372036854775808)
        (exact? (- (+ 1 10000000000000000000000000000000000)
                   10000000000000000000000000000000000))
        (exact? 10000000000000000000)))
(test-equal "decimals are inexact" '(#t #t #t #t)
  (list (inexact? 3.14) (inexact? 2.0) (inexact? 0.0) (inexact? -5.5)))

;; Exactness is syntactic: 123 and 123.0 are the same point on the line and
;; different numbers. The `.rs` test asserted `(exact? 123)` and
;; `(exact? 123.0)` twice each; the repeats are dropped. It also carried
;; `(= 123 123.0)` commented out, waiting on mixed comparison — which holds
;; now, so it is a case here.
(test-equal "a decimal point makes a literal inexact, not a different number"
  '(#t #f #f #t #t)
  (list (exact? 123) (inexact? 123) (exact? 123.0) (inexact? 123.0)
        (= 123 123.0)))

(test-equal "+ is inexact when any argument is" '(#t #t #t #t)
  (list (exact? (+ 1 2)) (inexact? (+ 1.0 2.0))
        (inexact? (+ 1 2.0)) (inexact? (+ 1.0 2))))
(test-equal "* is inexact when any argument is" '(#t #t #t #t)
  (list (exact? (* 3 4)) (inexact? (* 3.0 4.0))
        (inexact? (* 3 4.0)) (inexact? (* 3.0 4))))
(test-equal "- is inexact when any argument is" '(#t #t #t #t)
  (list (exact? (- 10 3)) (inexact? (- 10.0 3.0))
        (inexact? (- 10 3.0)) (inexact? (- 10.0 3))))
(test-equal "/ is inexact when any argument is" '(#t #t #t)
  (list (inexact? (/ 10.0 3.0)) (inexact? (/ 10 3.0)) (inexact? (/ 10.0 3))))
(test-equal "one inexact operand makes a whole expression inexact" '(#t #t #t)
  (list (exact? (+ (* 2 3) (- 10 5)))
        (inexact? (+ (* 2 3.0) (- 10 5)))
        (inexact? (+ (* 2 3) (- 10.0 5)))))

;; ── Bignums ────────────────────────────────────────────────────────────────

(test-equal "bignum arithmetic stays exact" '(#t #t #t)
  (list (exact? (+ 10000000000000000000 10000000000000000000))
        (exact? (* 10000000000000000000 2))
        (exact? (+ 9223372036854775807 1))))
(test-equal "overflowing a machine word promotes rather than wrapping"
  '(9223372036854775808 10000000000000000000)
  (list (+ 9223372036854775807 1) (* 1000000000 10000000000)))

;; Each result passes a 64-bit word, and 2^100 is built by repeated
;; multiplication rather than `expt`.
(test-equal "an iterative fib 100 is a bignum" 354224848179261915075
  (let ()
    (define (fib n)
      (define (fib-iter a b count)
        (if (= count 0) a (fib-iter b (+ a b) (- count 1))))
      (fib-iter 0 1 n))
    (fib 100)))
(test-equal "a recursive 25! is a bignum" 15511210043330985984000000
  (let ()
    (define (factorial n) (if (= n 0) 1 (* n (factorial (- n 1)))))
    (factorial 25)))
(test-equal "2^100 by repeated multiplication" 1267650600228229401496703205376
  (let ()
    (define (power base exp)
      (define (power-iter base exp acc)
        (if (= exp 0) acc (power-iter base (- exp 1) (* acc base))))
      (power-iter base exp 1))
    (power 2 100)))

;; ── Integer algorithms through multiple values ─────────────────────────────
;;
;; The `.rs` tests defined `gcd` at the top level of their own programs. Here
;; that would redefine an imported binding, so the helper is `euclid`, bound
;; locally. Their two programs are one row: the cases are the same algorithm.

(test-equal "Euclid's gcd, stepping with values" '(6 5 6 21)
  (let ()
    (define (quotient-and-remainder a b)
      (values (quotient a b) (remainder a b)))
    (define (euclid a b)
      (if (= b 0)
          a
          (let-values (((q r) (quotient-and-remainder a b)))
            (euclid b r))))
    (list (euclid 48 18) (euclid 100 35) (euclid 54 24) (euclid 1071 462))))

;; 48*(-1) + 18*3 = 6.
(test-equal "the extended Euclidean algorithm returns three values" '(6 -1 3)
  (let ()
    (define (extended-gcd a b)
      (if (= b 0)
          (values a 1 0)
          (let-values (((g x1 y1) (extended-gcd b (remainder a b))))
            (let ((q (quotient a b)))
              (values g y1 (- x1 (* q y1)))))))
    (call-with-values (lambda () (extended-gcd 48 18)) list)))

(test-equal "a = b*q + r for quotient and remainder" '((3 2 17) #t)
  (list (let-values (((q r) (values (quotient 17 5) (remainder 17 5))))
          (list q r (+ (* 5 q) r)))
        (let-values (((q r) (values (quotient 100 7) (remainder 100 7))))
          (= 100 (+ (* 7 q) r)))))

;; ── Rounding ───────────────────────────────────────────────────────────────
;;
;; An exact integer argument stays exact in all four; the inexact cases keep
;; the sign of a zero, which is why these are written rather than compared.

(test-equal "floor" '("3.0" "-4.0" "3.0" "-1.0" "5")
  (map written (list (floor 3.7) (floor -3.7) (floor 3.0) (floor -0.5) (floor 5))))

(test-equal "ceiling" '("4.0" "-3.0" "3.0" "-0.0" "5")
  (map written (list (ceiling 3.2) (ceiling -3.2) (ceiling 3.0) (ceiling -0.5) (ceiling 5))))

(test-equal "truncate rounds toward zero" '("3.0" "-3.0" "3.0" "-0.0" "5")
  (map written (list (truncate 3.7) (truncate -3.7) (truncate 3.0) (truncate -0.5) (truncate 5))))

;; Banker's rounding: 3.5 and 4.5 both go to 4.0, not 4.0 and 5.0.
(test-equal "round goes to even" '("4.0" "4.0" "4.0" "-4.0" "3.0" "5")
  (map written (list (round 3.5) (round 4.5) (round 3.7) (round -3.7) (round 3.2) (round 5))))

;; ── Rational accessors ─────────────────────────────────────────────────────

(test-equal "numerator, and that 6/8 is already simplified" '("3" "3" "5" "-2" "0")
  (map written (list (numerator 3/4) (numerator 6/8) (numerator 5)
                     (numerator -2/3) (numerator 0))))

(test-equal "denominator, and that an integer's is 1" '("4" "4" "1" "3")
  (map written (list (denominator 3/4) (denominator 6/8) (denominator 5)
                     (denominator -2/3))))

;; ── Exactness conversion ───────────────────────────────────────────────────

(test-equal "exact, including the ratio a float really is" '("3" "5/2" "1/4" "5" "1/2")
  (map written (list (exact 3.0) (exact 2.5) (exact 0.25) (exact 5) (exact 1/2))))

(test-equal "inexact" '("3.0" "2.5" "0.25" "3.0")
  (map written (list (inexact 3) (inexact 5/2) (inexact 1/4) (inexact 3.0))))

;; ── Roots and powers ───────────────────────────────────────────────────────
;;
;; `sqrt` here is always inexact, so `(sqrt 4)` is `2.0` where R7RS §6.2.6
;; says an exact argument with an exactly representable root should give an
;; exact result. **Both oracles return the exact `2`**, so this is our gap and
;; not a three-way difference of opinion — issue #225, classed `patina-defect`
;; in the register. The row pins what we do today rather than what we should
;; do, deliberately: fixing `sqrt` will fail it, which is the signal to update
;; the expectation and retire the register entries in the same change.
(test-equal "sqrt is inexact even for exact squares" '("2.0" "3.0" "1.4142135623730951" "0.0")
  (map written (list (sqrt 4) (sqrt 9) (sqrt 2) (sqrt 0))))

(test-equal "expt, including a negative exponent giving a ratio"
  '("8" "25" "1" "1/10" "8.0" "8.0" "1" "1")
  (map written (list (expt 2 3) (expt 5 2) (expt 2 0) (expt 10 -1)
                     (expt 2.0 3) (expt 2 3.0)
                     (expt 0 0)      ; 1 by convention
                     (expt 1 1000))))

(test-equal "square" '("16" "25" "0" "2.25")
  (map written (list (square 4) (square -5) (square 0) (square 1.5))))

;; ── Number theory ──────────────────────────────────────────────────────────
;;
;; Both take any number of arguments, including none, and both ignore the sign.

(test-equal "gcd" '("4" "6" "1" "5" "5" "4" "6" "0")
  (map written (list (gcd 12 8) (gcd 18 24) (gcd 7 13) (gcd 0 5) (gcd 5 0)
                     (gcd -12 8) (gcd 12 18 24) (gcd))))

(test-equal "lcm" '("12" "36" "91" "0" "12" "12" "1")
  (map written (list (lcm 4 6) (lcm 12 18) (lcm 7 13) (lcm 0 5) (lcm -4 6)
                     (lcm 2 3 4) (lcm))))

;; Two values, s and r, with k = s² + r. The `.rs` rows compared the two
;; joined by a newline, which was `display_tagged`'s rendering of a values
;; object rather than anything a program can see; `call-with-values` is the
;; portable way to ask.
(define (isqrt k) (call-with-values (lambda () (exact-integer-sqrt k)) list))
(test-equal "exact-integer-sqrt returns s and r with k = s*s + r"
  '((2 0) (2 1) (3 0) (3 1) (0 0))
  (list (isqrt 4) (isqrt 5) (isqrt 9) (isqrt 10) (isqrt 0)))

;; R7RS: an inexact argument gives an inexact result, an exact one an exact
;; result — so the same approximation appears twice in different clothes.
;; `(rationalize 314159/100000 1/100)` is the classic 22/7.
(test-equal "rationalize keeps the exactness of its argument"
  '("3.142857142857143" "1.5" "0.3333333333333333" "22/7" "3/2" "1/3")
  (map written (list (rationalize 3.14159 0.01) (rationalize 1.5 0.1)
                     (rationalize 0.333 0.01)
                     (rationalize 314159/100000 1/100) (rationalize 3/2 1/10)
                     (rationalize 333/1000 1/100))))

;; **Larceny family 8** (`scheme_tests/reports/larceny_triage.md`), moved here
;; from `larceny_families.rs` to sit beside the `rationalize` row above.
;;
;; R7RS §6.2.6 at the infinities: `(rationalize +inf.0 3)` is `+inf.0`, and
;; `(rationalize 3 +inf.0)` is `0.0` — inexact, because the tolerance is. The
;; exactness is the point of writing these rather than comparing them: an exact
;; `0` would be a different answer. Fixed 2026-08-24.
(test-equal "rationalize at the infinities" '("+inf.0" "0.0" "-inf.0")
  (map written (list (rationalize +inf.0 3) (rationalize 3 +inf.0)
                     (rationalize -inf.0 1))))

;; ── Float predicates ───────────────────────────────────────────────────────

(test-equal "finite?" '(#t #t #t #f #f #f)
  (list (finite? 3) (finite? 3.14) (finite? 1/2)
        (finite? +inf.0) (finite? -inf.0) (finite? +nan.0)))

(test-equal "infinite?" '(#f #f #t #t #f)
  (list (infinite? 3) (infinite? 3.14) (infinite? +inf.0)
        (infinite? -inf.0) (infinite? +nan.0)))

(test-equal "nan?, including the one you have to compute" '(#f #f #f #f #t #t)
  (list (nan? 3) (nan? 3.14) (nan? +inf.0) (nan? -inf.0) (nan? +nan.0)
        (nan? (/ 0.0 0.0))))

;; `6/2` is the interesting one: it simplifies to an exact 3 and so qualifies,
;; where `3.0` and `3/2` do not.
(test-equal "exact-integer?" '(#t #f #f #t)
  (list (exact-integer? 3) (exact-integer? 3.0)
        (exact-integer? 3/2) (exact-integer? 6/2)))

;; ── Complex accessors ──────────────────────────────────────────────────────
;;
;; Exact parts stay exact and inexact parts stay inexact, which is why these
;; are written: `3` and `3.0` are the same point on the line and different
;; numbers here.

(test-equal "real-part" '("3" "3.0" "5" "0" "-2")
  (map written (list (real-part 3+4i) (real-part 3.0+4.0i) (real-part 5)
                     (real-part +i) (real-part -2-3i))))

(test-equal "imag-part, and that a real number has an exact zero one"
  '("4" "4.0" "0" "1" "-3")
  (map written (list (imag-part 3+4i) (imag-part 3.0+4.0i) (imag-part 5)
                     (imag-part +i) (imag-part -2-3i))))

;; Same gap as `sqrt`, same issue #225: both oracles answer the exact `5` to
;; `(magnitude 5)`.
(test-equal "magnitude is inexact even for a 3-4-5 triangle" '("5.0" "5.0" "5.0" "1.0")
  (map written (list (magnitude 3+4i) (magnitude 5) (magnitude -5) (magnitude +i))))

;; A zero imaginary part collapses to a real; a zero real part is elided.
(test-equal "make-rectangular" '("3+4i" "5" "+i" "-2-3i")
  (map written (list (make-rectangular 3 4) (make-rectangular 5 0)
                     (make-rectangular 0 1) (make-rectangular -2 -3))))

;; The `.rs` row accepted `"1"` or `"1.0"` because it could not say "either
;; exactness, but the right number". A value comparison can.
(test-assert "make-polar at angle zero is 1, of whichever exactness"
  (close? (make-polar 1 0) 1))

;; ── Complex equality, printing and angle ───────────────────────────────────
;;
;; From `compliance/numeric_edge_cases.rs` and `compliance/numbers.rs`.

;; `1+0i` reads as the exact real 1, so it equals 1 and 1.0. The `.rs` file had
;; a second test, meant for ordering on complex numbers, that asserted only
;; `(= 1+2i 1+2i)` again; it is the first case here.
(test-equal "= on complex numbers, where a zero imaginary part is real"
  '(#t #f #t #t #t #f)
  (list (= 1+2i 1+2i) (= 1+2i 3+4i) (= 1+0i 1.0) (= 1+0i 1) (= 1.0 1+0i 1)
        (= 1+2i 1.0)))

;; §6.2.6: `(string->number (number->string z))` must be equivalent to `z`,
;; which a complex with a zero real part did not satisfy. R7RS 7.1.1 spells an
;; imaginary part `<sign> <ureal R> i`, so the sign is syntax and not
;; decoration: `number->string` produced `"2.0i"`, which is not a number at
;; all. And a bare `<imaginary R>` reads with an *exact* zero real part, so
;; `+2.0i` cannot stand for `(make-rectangular 0.0 2.0)` — the inexact zero
;; has to be written.
(test-equal "number->string writes an inexact zero real part"
  '("0.0+2.0i" "0.0-2.0i" "0.0+1.0i" "1.0-1.0i")
  (map number->string
       (list (make-rectangular 0.0 2.0) (make-rectangular 0.0 -2.0)
             (make-rectangular 0.0 1.0) (make-rectangular 1.0 -1.0))))

;; An infinite or NaN part formats with its own sign, so it needs the same
;; guard as the pure-imaginary case — it used to concatenate a second `+` and
;; produce `1.0++inf.0i`, which is not a number.
(test-equal "number->string with an infinite imaginary part"
  '("1.0+inf.0i" "0.0+inf.0i")
  (map number->string
       (list (make-rectangular 1.0 +inf.0) (make-rectangular 0.0 +inf.0))))

;; With an *exact* zero real part, which R7RS lets the writer elide. chibi
;; writes it as `0`, which reads back just as well, and Gauche has no
;; mixed-exactness complex numbers to write, so the spelling is Patina's —
;; the same call as "how complex results are written" below. A unit imaginary
;; part is its own case: `+i` reads back *exact*, so collapsing an inexact 1.0
;; to it would lose the inexactness.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "Patina elides an exact zero real part"
  '("+2.0i" "-2.0i" "+1.0i" "+i")
  (map number->string
       (list (make-rectangular 0 2.0) (make-rectangular 0 -2.0)
             (make-rectangular 0 1.0) (make-rectangular 0 1))))

;; An inexact zero *imaginary* part is the mirror image: R7RS §6.2.6 has
;; `(real? -2.5+0.0i)` answer #f, so writing it as a real would name a
;; different number. Gauche reads `1.0+0.0i` as the real 1.0 (registered as
;; latitude: it has no such complex number to keep).
(test-equal "number->string keeps an inexact zero imaginary part" "1.0+0.0i"
  (number->string (string->number "1.0+0.0i")))

;; The round trip R7RS actually requires, across the shapes that exercise each
;; arm above rather than only the easy one.
(test-equal "complex numbers round-trip through number->string"
  '(#t #t #t #t #t #t #t)
  (let ((round-trips? (lambda (z) (equal? z (string->number (number->string z))))))
    (map round-trips?
         (list (make-rectangular 0.0 2.0) (make-rectangular 0 2.0)
               (make-rectangular 1.0 2.0) (make-rectangular 0.0 1.0)
               (make-rectangular 1.0 -1.0) (make-rectangular 1.0 +inf.0)
               (make-rectangular 0.0 +inf.0)))))

;; `write` must agree with `number->string`; it used to omit the inexact zero
;; too, which is how the `sqrt` inexactness stayed invisible. The `.rs` row
;; compared both against Patina's spelling; agreement is the claim, and it is
;; one every implementation can be asked.
(test-equal "write agrees with number->string on complex numbers" '(#t #t #t #t)
  (map (lambda (z) (string=? (written z) (number->string z)))
       (list (make-rectangular 0.0 2.0) (make-rectangular 0 2.0)
             (make-rectangular 0.0 1.0) (make-rectangular 0 1))))

;; §6.2.6: `angle` of a negative real is pi, and of a positive one zero —
;; inexact, as every implementation here answers even for exact arguments.
;; Bignums reach it through a conversion to a float, so they are cases.
(test-equal "angle of a negative real is pi"
  '(3.141592653589793 3.141592653589793 3.141592653589793 3.141592653589793
    3.141592653589793)
  (list (angle -1.0) (angle -5) (angle -1/2) (angle -inf.0)
        (angle (- (expt 2 5000)))))
(test-equal "angle of a positive real or zero is 0.0" '(0.0 0.0 0.0 0.0 0.0 0.0)
  (list (angle 0.0) (angle 1.0) (angle 5) (angle (expt 2 5000)) (angle +inf.0)
        (angle 0)))
(test-equal "angle of i is pi/2" 1.5707963267948966 (angle +i))

;; `-0.0` is negative by its *sign bit*, not by ordering — `(negative? -0.0)`
;; is #f and must stay so — and asking the ordering question answered 0.0. A
;; real is the complex number with a +0.0 imaginary part, so the answer is
;; `atan2(+0.0, -0.0)`, which IEEE 754 §9.2 makes pi, as C's `carg` and
;; chibi do. Gauche answers 0.0 (registered as spec-silent).
(test-equal "angle of -0.0 is pi" 3.141592653589793 (angle -0.0))
(test-equal "-0.0 is zero and not negative" '(#f #t)
  (list (negative? -0.0) (zero? -0.0)))

;; `+nan.0` used to answer 0.0, and a non-number did too, because the
;; predicate this replaced was documented total. Gauche answers pi for the
;; NaN (registered as spec-silent).
(test-assert "angle of a NaN is a NaN" (nan? (angle +nan.0)))
(test-error "angle of a non-number is an error" #t (angle "x"))

;; ── Trigonometric ──────────────────────────────────────────────────────────

(test-equal "sin, cos and tan at zero" '("0.0" "1.0" "0.0")
  (map written (list (sin 0) (cos 0) (tan 0))))

(test-equal "asin" '("0.0" "1.5707963267948966")     ; the second is pi/2
  (map written (list (asin 0) (asin 1))))

(test-equal "acos" '("0.0" "1.5707963267948966")     ; pi/2 again
  (map written (list (acos 1) (acos 0))))

;; Including the two-argument form, which is the one that knows its quadrant.
(test-equal "atan" '("0.0" "0.7853981633974483" "0.0" "1.5707963267948966")
  (map written (list (atan 0) (atan 1) (atan 0 1) (atan 1 0))))

;; ── Exponential and logarithmic ────────────────────────────────────────────

(test-equal "exp" '("1.0" "2.718281828459045")
  (map written (list (exp 0) (exp 1))))

;; Two-argument `log` takes a base, so these are log_2(8) and log_10(100).
(test-equal "log, including the two-argument form" '("0.0" "1.0" "3.0" "2.0")
  (map written (list (log 1) (log 2.718281828459045) (log 8 2) (log 100 10))))

;; ── Complex results from real arguments ────────────────────────────────────
;;
;; `sqrt` always returns an inexact result, so the real part of a pure
;; imaginary answer is `0.0` and has to be written: `+1.0i` would read back
;; with an *exact* zero real part, which is a different number. These
;; expectations were `+1.0i` until the writer stopped omitting an inexact zero
;; real part, which is what made the inexactness visible.
(test-equal "sqrt of a negative, and both branch cuts"
  '("0.0+1.0i" "0.0+1.0i" "0.0+1.0i" "0.0+1.0i" "0.0+1.0i"
    "1.5537739740300374+0.6435942529055827i")
  (map written (list (sqrt -1) (sqrt -1.0)
                     (sqrt -1.0-0.0i)   ; approaching the cut from below
                     (sqrt -1.0+0.0i)   ; and from above
                     (inexact (sqrt -1))
                     (sqrt 2+2i))))

;; The six rows below replace substring assertions. Each `.rs` row checked
;; that the printed form contained a `+`, an `i`, or two or three digits —
;; which `"+inf.0"` would also satisfy. The identities are exact, so the
;; values can simply be named.
(test-assert "exp of i is cos 1 + i sin 1"
  (let ((z (exp 0+1i)))
    (and (close? (real-part z) (cos 1)) (close? (imag-part z) (sin 1)))))

(test-assert "log of -1 is i*pi"
  (let ((z (log -1)))
    (and (close? (real-part z) 0) (close? (imag-part z) (atan 0 -1)))))

(test-assert "sin of i is i*sinh 1"
  (let ((z (sin 0+1i)))
    (and (close? (real-part z) 0)
         (close? (imag-part z) (/ (- (exp 1) (exp -1)) 2)))))

(test-assert "cos of i is cosh 1"
  (close? (real-part (cos 0+1i)) (/ (+ (exp 1) (exp -1)) 2)))

;; asin of a real greater than 1 leaves the real line: pi/2 - i*arcosh(2), and
;; arcosh(2) is ln(2+sqrt 3), which is portable arithmetic rather than a
;; constant to paste. Named rather than asserted to be merely non-zero: "not
;; close to 0" is satisfied by any wrong value, and by +nan.0, since `close?`
;; answers #f there and the `not` then passes the row.
(test-assert "asin of 2 is pi/2 minus i arcosh 2"
  (let ((z (asin 2)))
    (and (close? (real-part z) (/ (atan 0 -1) 2))
         (close? (imag-part z) (- (log (+ 2 (sqrt 3))))))))

;; `(atan 0+1i)` is a pole of the principal branch, where R7RS defines no
;; value. Measured 2026-09-07, the four implementations do four things:
;;
;;   patina   0.0+inf.0i
;;   Gauche   +nan.0+inf.0i
;;   Chez     raises — "Exception in atan: undefined for 0+1i"
;;   chibi    **segfaults** — exit 139, not a catchable error
;;
;; chibi's answer is why this row is scoped, and its crash is *not* about the
;; pole: `(atan 0+2i)` is an ordinary point and crashes too. The trigger is an
;; exact complex with an exact zero real part — `0+1i`, `0-1i`, `0+2i`, `0+3i`,
;; `(make-rectangular 0 2)` all die, while `1+1i`, `2+0i` and the inexact
;; `0.0+2.0i` all answer normally. Reproduced on chibi's master at
;; 0.12-201-g6991e209, not only on the 0.12 release. Tracked in issue #227 and
;; filed upstream as ashinn/chibi-scheme#1197, where it was **fixed 2026-09-09**
;; by commit cd989e2e4439, "account for normalization to non-complex in
;; sexp_complex_atan" (bignum.c). The scope stays until the chibi the lane runs
;; carries that fix — 0.12.0 as installed does not — and comes off when it does,
;; at which point chibi arbitrates a 37th row here.
;;
;; Without the skip it takes the whole file down and chibi reports nothing at
;; all, because SRFI 64's summary only prints at `test-end` and never arrives —
;; 36 passing rows lost to one that asserts almost nothing. A **reported skip**
;; rather than a bare `cond-expand` so the row cannot vanish quietly, and
;; verified that `test-skip` prevents *evaluation* rather than merely discarding
;; the result, which is the property this depends on.
;;
;; The `.rs` row asserted only that the call returned. Since this one is scoped
;; to Patina anyway, it can name our answer instead — a scope, a register entry
;; and an upstream report have all been spent on this row, and `#t` would be
;; satisfied by any regression that still returned something.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "atan of i is our 0.0+inf.0i" "0.0+inf.0i" (written (atan 0+1i)))

;; ── Complex arithmetic ─────────────────────────────────────────────────────
;;
;; Moved from `crates/patina-tests/tests/complex_numbers.rs` (#193 Phase 2),
;; which is deleted. It built a `TreeWalkInterpreter` by hand for every one of
;; its 25 tests, so none had run on the VM, and it compared printed text.
;; Here arithmetic is compared as numbers — `equal?` where every result is a
;; non-real complex, `=` where a result collapses to a real, since Gauche has
;; no exact complex numbers and answers 0.0 where the rest answer 0 — and the
;; two claims that are *not* about value get rows of their own: that a zero
;; imaginary part leaves an exact real, and how Patina writes the results.

(test-equal "complex addition and subtraction"
  '(4+6i 3+2i 6+6i 8+4i 12+3i 3+4i 3+4i 3+4i)
  (list (+ 1+2i 3+4i) (+ 5+3i -2-1i) (+ 1+1i 2+2i 3+3i)
        (+ 3+4i 5) (+ 10 2+3i) (- 5+7i 2+3i)
        (+ 0 3+4i) (+ 3+4i 0)))

(test-equal "complex multiplication and negation"
  '(5+i 4+6i 3+6i -3-4i 5-2i -i +2i -2i +12i)
  (list (* 2+3i 1-1i) (* 2+3i 2) (* 3 1+2i)
        (- 3+4i) (- -5+2i) (- +i)
        (* 1+1i 1+1i) (* 1-1i 1-1i)
        (* (+ 1+i 2+2i) (- 3+3i 1+1i))))

;; Results whose imaginary part cancels, compared as numbers.
(test-equal "products and differences that cancel to reals"
  '(#t #t #t #t #t #t #t #t)
  (map = (list (* 1+i 1-i) (* +i +i) (* -i -i) (* (* +i +i) +i)
               (* (* +i +i) (* +i +i)) (* 3+4i 3-4i)
               (- 10+5i 10+5i) (+ 3+4i -3-4i))
       '(2 -1 -1 -i 1 25 0 0)))

;; With exact parts, a zero imaginary part leaves an exact *real*: R7RS
;; §6.2.6 has `(real? -2.5+0i)` answer #t. Gauche has no exact complex
;; numbers, so its results are inexact (registered as latitude).
(test-equal "an exact complex whose imaginary part cancels is an exact real"
  '((#t #t) (#t #t) (#t #t) (#t #t) (#t #t))
  (map (lambda (x) (list (real? x) (exact? x)))
       (list (* 1+i 1-i) (* +i +i) (- 10+5i 10+5i) (* 3+4i 3-4i) (* 0 3+4i))))

;; The algebra holds: a difference of squares, distribution, association.
(test-equal "complex arithmetic obeys the field identities" '(#t #t #t)
  (let ((z 3+4i) (w 1+2i) (a 2+3i) (b 1+1i) (c 4-2i))
    (list (= (* (- z w) (+ z w)) (- (* z z) (* w w)))
          (= (* a (+ b c)) (+ (* a b) (* a c)))
          (= (* (* 1+2i 3-1i) 2+2i) (* 1+2i (* 3-1i 2+2i))))))

;; Iterations that stay complex, or pass through the reals and back.
(test-equal "iterated complex arithmetic" '(#t #t #t #t #t #t #t #t #t)
  (let ()
    (define (complex-fib n a b)
      (if (= n 0) a (if (= n 1) b (complex-fib (- n 1) b (+ a b)))))
    (define (julia z c n)
      (if (= n 0) z (julia (+ (* z z) c) c (- n 1))))
    (map = (list (complex-fib 0 0+0i 0+1i) (complex-fib 1 0+0i 0+1i)
                 (complex-fib 3 0+0i 0+1i) (complex-fib 5 0+0i 0+1i)
                 (complex-fib 2 1+1i 1-1i) (complex-fib 5 1+1i 1-1i)
                 (julia 2+0i 0+0i 1) (julia 2+0i 0+0i 2) (julia 0+1i 0+0i 1))
         '(0 +i +2i +5i 2 8-2i 4 16 -1))))

;; Literal syntax with inexact and rational parts, and in polar form.
(test-equal "complex literals with inexact and rational parts"
  '(3.5 2.7 1.0 -0.5 #t #t)
  (list (real-part 3.5+2.7i) (imag-part 3.5+2.7i)
        (real-part 1.0-0.5i) (imag-part 1.0-0.5i)
        (= (real-part 1/2+3/4i) 1/2) (= (imag-part 1/2+3/4i) 3/4)))

(test-equal "polar literals" '(#t #t #t)
  (let ((close? (lambda (a b) (< (abs (- a b)) 1e-4))))
    (list (close? (real-part 1@0) 1)
          (close? (imag-part 1@1.5708) 1)
          (close? (magnitude (+ 1 (+ -0.5+0.866i -0.5-0.866i))) 0))))

;; How Patina writes these results — the elided unit and zero parts, and the
;; collapse to a plain real. R7RS leaves the spelling to the writer, and the
;; oracles elide differently (see "make-rectangular" above), so this row is
;; Patina's.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "how complex results are written"
  '("-1-i" "+i" "-i" "+5i" "-3i" "3+i" "5-i" "3.5+2.7i" "1.0-0.5i"
    "+2i" "-2i" "+12i" "8-2i" "0" "25")
  (map written (list -1-1i +i -i +5i -3i 3+i 5-i 3.5+2.7i 1.0-0.5i
                     (* 1+1i 1+1i) (* 1-1i 1-1i) (* (+ 1+i 2+2i) (- 3+3i 1+1i))
                     8-2i (- 10+5i 10+5i) (* 3+4i 3-4i))))

(test-end)
