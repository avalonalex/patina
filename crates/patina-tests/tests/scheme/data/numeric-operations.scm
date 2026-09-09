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

(test-end)
