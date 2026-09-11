;; Exact rationals: reading, normalizing, arithmetic, comparison, and where
;; exactness is lost.
;;
;; Migrated from `crates/patina-tests/tests/compliance/rationals.rs` (#193),
;; which is deleted: 38 tests over 129 assertions, one row per test here, each
;; carrying that test's cases as a list so the expected value names which
;; case drifted. `data/numeric-operations.scm` holds the accessors
;; (`numerator`, `denominator`, `rationalize`) and the exactness conversions.
;;
;; **Why most rows are written rather than compared.** A ratio's printed form
;; is where normalization shows: `equal?` holds between 1/2 and an
;; unnormalized 100/200, since both are the same number, but only one of them
;; prints as "1/2". The `.rs` helper compared that printed form, so the rows
;; that are about a ratio's value keep the writer under test with the
;; `written` helper `docs/TEST_ORGANIZATION.md` records. Rows whose results
;; are booleans compare them directly; those print only one way.

(import (scheme base) (scheme write) (srfi 64))

(define (written x) (let ((p (open-output-string))) (write x p) (get-output-string p)))

(test-begin "rationals")

;; ── Literals and normalization ─────────────────────────────────────────────

(test-equal "ratio literals read in lowest terms" '("1/2" "3/4" "5/6" "1/2")
  (map written (list 1/2 3/4 5/6 100/200)))

(test-equal "a literal not in lowest terms is simplified" '("1/2" "1/2" "1/2" "1/3" "3/7")
  (map written (list 2/4 4/8 10/20 15/45 21/49)))

(test-equal "a literal with a unit denominator is an integer" '("2" "2" "5" "10")
  (map written (list 6/3 10/5 15/3 100/10)))

(test-equal "negative ratios" '("-1/2" "-1/2" "-3/4")
  (map written (list -1/2 (- 1/2) -3/4)))

(test-equal "improper fractions stay ratios" '("5/3" "7/4" "41/12")
  (map written (list 5/3 7/4 (+ 5/3 7/4))))

;; ── Arithmetic ─────────────────────────────────────────────────────────────

(test-equal "rational addition" '("5/6" "1/2" "1" "5/6" "1")
  (map written (list (+ 1/2 1/3) (+ 1/4 1/4) (+ 1/3 1/3 1/3) (+ 2/3 1/6)
                     (+ 1/2 1/2))))

(test-equal "rational subtraction" '("1/2" "1/2" "1/2" "1/2" "-1/2")
  (map written (list (- 5/6 1/3) (- 3/4 1/4) (- 2/3 1/6) (- 1 1/2) (- 1/2 1))))

(test-equal "rational multiplication" '("1/6" "1/2" "3/5" "1" "1")
  (map written (list (* 1/2 1/3) (* 2/3 3/4) (* 3/4 4/5) (* 1/2 2) (* 2/3 3/2))))

(test-equal "rational division, and integer division giving a ratio"
  '("3/2" "5/6" "1/2" "1/3" "3/4")
  (map written (list (/ 1/2 1/3) (/ 2/3 4/5) (/ 1 2) (/ 1 3) (/ 3 4))))

(test-equal "integers and ratios mixed" '("3/2" "5/3" "2" "4/3")
  (map written (list (+ 1 1/2) (- 2 1/3) (* 3 2/3) (/ 4 3))))

(test-equal "nested rational expressions" '("1" "2/3" "1/2" "1/2")
  (map written (list (+ (/ 1 2) (/ 1 3) (/ 1 6)) (- (+ 1/2 1/3) 1/6)
                     (* (+ 1/2 1/2) (- 2/3 1/6)) (/ (+ 1/2 1/2) 2))))

(test-equal "ratios with small denominators" '("1/500" "1/10000")
  (map written (list (+ 1/1000 1/1000) (* 1/100 1/100))))

(test-equal "ratios with a large denominator" "1/500000"
  (written (+ 1/1000000 1/1000000)))

;; A one-argument `/` is the reciprocal.
(test-equal "the reciprocal of a ratio" '("2" "3" "3/2" "4/3")
  (map written (list (/ 1/2) (/ 1/3) (/ 2/3) (/ 3/4))))

(test-equal "zero with a ratio" '("1/2" "1/2" "0")
  (map written (list (+ 1/2 0) (- 1/2 0) (* 1/2 0))))

(test-equal "one with a ratio" '("1/2" "1/2" "1")
  (map written (list (* 1/2 1) (/ 1/2 1) (+ 1/2 1/2))))

(test-equal "negative ratios in arithmetic" '("0" "-1" "-1/2" "-1/2")
  (map written (list (+ -1/2 1/2) (* -1/2 2) (/ -1 2) (- 0 1/2))))

(test-equal "two negatives" '("1/6" "1/6" "3/2")
  (map written (list (+ 1/2 -1/3) (* -1/2 -1/3) (/ -1/2 -1/3))))

(test-equal "chains of more than two operands" '("77/60" "1/120")
  (map written (list (+ 1/2 1/3 1/4 1/5) (* 1/2 1/3 1/4 1/5))))

;; The `.rs` test's name said these cases were checked against chibi.
(test-equal "a mixed set of rational operations" '("5/6" "1/2" "1/2" "4/3" "#t" "#t")
  (map written (list (+ 1/2 1/3) (- 5/6 1/3) (* 2/3 3/4) (/ 8/9 2/3)
                     (= 2/4 1/2) (< 1/3 1/2 2/3))))

;; `(/ 1 0)` was here too; it is a row in `data/numeric-operations.scm`,
;; identical, so only the ratio dividend is new.
(test-error "a ratio divided by exact zero is an error" #t (/ 1/2 0))

;; ── Comparisons ────────────────────────────────────────────────────────────

(test-equal "= on ratios, including against an inexact equal" '(#t #t #t #f #t)
  (list (= 1/2 1/2) (= 2/4 1/2) (= 3/6 1/2) (= 1/2 1/3) (= 1/2 0.5)))

;; The last case in each of these two is new: the `.rs` cases had no equal
;; pair, so a `<` that answered `<=` passed them all.
(test-equal "< on ratios" '(#t #t #f #t #f)
  (list (< 1/3 1/2) (< 1/4 1/3 1/2) (< 1/2 1/3) (< 2/5 1/2 3/5) (< 1/2 2/4)))

(test-equal "> on ratios" '(#t #t #f #t #f)
  (list (> 1/2 1/3) (> 3/4 1/2 1/4) (> 1/3 1/2) (> 2/3 1/2 1/3) (> 1/2 2/4)))

(test-equal "<= on ratios" '(#t #t #f #t)
  (list (<= 1/3 1/2) (<= 1/2 1/2) (<= 1/2 1/3) (<= 1/4 1/3 1/2 1/2)))

(test-equal ">= on ratios" '(#t #t #f #t)
  (list (>= 1/2 1/3) (>= 1/2 1/2) (>= 1/3 1/2) (>= 2/3 1/2 1/2 1/3)))

;; ── Exactness ──────────────────────────────────────────────────────────────

(test-equal "ratios are exact, computed ones too" '(#t #t #t #t)
  (list (exact? 1/2) (exact? 3/4) (exact? (/ 1 2)) (exact? (+ 1/2 1/3))))

(test-equal "a ratio with a float gives a float" '("1.0" "1.0" "1.0" "0.25")
  (map written (list (+ 1/2 0.5) (+ 0.5 1/2) (* 2/3 1.5) (- 1/2 0.25))))

(test-equal "a ratio with a float is inexact" '(#t #t #f)
  (list (inexact? (+ 1/2 0.5)) (inexact? (* 2/3 1.5)) (exact? (+ 1/2 0.5))))

(test-equal "dividing exact integers stays exact; a float operand does not"
  '(#t #t #t #t #t)
  (list (exact? (/ 1 2)) (exact? (/ 1 3)) (exact? (/ 22 7))
        (inexact? (/ 1.0 2)) (inexact? (/ 1 2.0))))

;; `2/1` reads as the integer 2, so it is one.
(test-equal "number? and integer? on ratios" '(#t #f #t)
  (list (number? 1/2) (integer? 1/2) (integer? 2/1)))

;; ── Field laws, exactly ────────────────────────────────────────────────────
;;
;; Exact arithmetic makes these equalities rather than approximations. With
;; binary64 floats the first row would fail — (0.5 + 1/3) + 1/6 sums to
;; 0.9999999999999999 — and so would the sevenths in the last, which sum to
;; 0.9999999999999998.

(test-assert "addition of ratios is associative"
  (= (+ (+ 1/2 1/3) 1/6) (+ 1/2 (+ 1/3 1/6))))

(test-equal "addition and multiplication of ratios commute" '(#t #t)
  (list (= (+ 1/2 1/3) (+ 1/3 1/2)) (= (* 2/3 3/4) (* 3/4 2/3))))

(test-assert "multiplication distributes over addition"
  (= (* 1/2 (+ 1/3 1/4)) (+ (* 1/2 1/3) (* 1/2 1/4))))

(test-equal "0 and 1 are the identities" '(#t #t)
  (list (= (+ 1/2 0) 1/2) (= (* 1/2 1) 1/2)))

(test-equal "additive inverses" '(#t #t)
  (list (= (+ 1/2 -1/2) 0) (= (+ 2/3 (- 2/3)) 0)))

(test-equal "multiplicative inverses" '(#t #t)
  (list (= (* 2/3 3/2) 1) (= (* 1/2 (/ 1/2)) 1)))

(test-equal "n copies of 1/n sum to exactly 1" '(#t #t)
  (list (= (+ 1/3 1/3 1/3) 1) (= (+ 1/7 1/7 1/7 1/7 1/7 1/7 1/7) 1)))

(test-end)
