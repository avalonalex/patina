;; SRFI 141 / Tangerine integer division (#576).
;; These rows use valid integer inputs. Patina's deliberate rejection of
;; invalid domains is tested in r7rs_large_aliases.rs, since SRFI 141's
;; "it is an error" does not require another implementation to signal one.
;; Chibi 0.12 fails the large signed grid because / and floor-quotient can
;; mutate a bignum numerator's sign when dividing by a negative fixnum.
;; Patina, Gauche 0.9.15 and Chez 10.3 preserve it (#576, 2026-09-30).
;; The investigated difference is recorded in DIVERGENCES.tsv.
(import (scheme base) (scheme division) (srfi 64))

(test-begin "division")

;; Each family is (name two-values quotient remainder).
(define families
  (list (list 'floor floor/ floor-quotient floor-remainder)
        (list 'ceiling ceiling/ ceiling-quotient ceiling-remainder)
        (list 'truncate truncate/ truncate-quotient truncate-remainder)
        (list 'round round/ round-quotient round-remainder)
        (list 'euclidean euclidean/ euclidean-quotient euclidean-remainder)
        (list 'balanced balanced/ balanced-quotient balanced-remainder)))

(define (pair-result proc n d)
  (call-with-values (lambda () (proc n d)) list))
(define (results n d)
  (map (lambda (family) (pair-result (cadr family) n d)) families))
(define (all? predicate xs)
  (or (null? xs)
      (and (predicate (car xs)) (all? predicate (cdr xs)))))

(test-equal "positive numerator and denominator"
  '((3 1) (4 -3) (3 1) (3 1) (3 1) (3 1)) (results 13 4))
(test-equal "negative numerator"
  '((-4 3) (-3 -1) (-3 -1) (-3 -1) (-4 3) (-3 -1)) (results -13 4))
(test-equal "negative denominator"
  '((-4 -3) (-3 1) (-3 1) (-3 1) (-3 1) (-3 1)) (results 13 -4))
(test-equal "both operands negative"
  '((3 -1) (4 3) (3 -1) (3 -1) (4 3) (3 -1)) (results -13 -4))

(test-assert "zero numerator"
  (all? (lambda (d) (equal? (results 0 d) (make-list 6 '(0 0)))) '(4 -4)))
(test-assert "unit denominators"
  (all? (lambda (n)
          (and (equal? (results n 1) (make-list 6 (list n 0)))
               (equal? (results n -1) (make-list 6 (list (- n) 0)))))
        '(13 -13 0)))
(test-assert "exactly divisible operands"
  (all? (lambda (args)
          (equal? (results (car args) (cadr args))
                  (make-list 6 (list (/ (car args) (cadr args)) 0))))
        '((12 4) (-12 4) (12 -4) (-12 -4))))

(define ties '((5 2) (7 2) (-5 2) (-7 2) (5 -2) (7 -2) (-5 -2) (-7 -2)))
(test-equal "round ties choose an even quotient"
  '((2 1) (4 -1) (-2 -1) (-4 1) (-2 1) (-4 -1) (2 -1) (4 1))
  (map (lambda (args) (pair-result round/ (car args) (cadr args))) ties))
(test-equal "balanced ties keep the negative half remainder"
  '((3 -1) (4 -1) (-2 -1) (-3 -1) (-3 -1) (-4 -1) (2 -1) (3 -1))
  (map (lambda (args) (pair-result balanced/ (car args) (cadr args))) ties))

(define inexact-results '((3.0 1.0) (4.0 -3.0) (3.0 1.0)
                          (3.0 1.0) (3.0 1.0) (3.0 1.0)))
(test-equal "inexact numerator" inexact-results (results 13.0 4))
(test-equal "inexact denominator" inexact-results (results 13 4.0))
(test-equal "both operands inexact" inexact-results (results 13.0 4.0))

;; Interval rules and n = dq + r uniquely specify every result. Checking
;; the scalar procedures as well catches implementations whose three entry
;; points disagree. Round additionally needs the even-quotient tie rule.
(define (family-correct? family n d)
  (let* ((name (car family))
         (qr (pair-result (cadr family) n d))
         (q (car qr)) (r (cadr qr)))
    (and (= n (+ (* d q) r))
         (integer? q) (< (abs r) (abs d))
         (= q ((car (cddr family)) n d))
         (= r ((cadr (cddr family)) n d))
         (if (and (exact? n) (exact? d))
             (and (exact? q) (exact? r))
             (and (inexact? q) (inexact? r)))
         (case name
           ((floor) (= q (floor (/ n d))))
           ((ceiling) (= q (ceiling (/ n d))))
           ((truncate) (= q (truncate (/ n d))))
           ((round) (and (<= (* 2 (abs r)) (abs d))
                         (or (< (* 2 (abs r)) (abs d)) (even? q))))
           ((euclidean) (>= r 0))
           ((balanced) (and (<= (- (abs d)) (* 2 r))
                            (< (* 2 r) (abs d))))))))
(define (grid-correct? ns ds)
  (all? (lambda (n)
          (all? (lambda (d)
                  (all? (lambda (family) (family-correct? family n d)) families))
                ds))
        ns))
(define small-ns '(-15 -14 -13 -7 -6 -5 -2 -1 0 1 2 5 6 7 13 14 15))
(define small-ds '(-7 -4 -3 -2 -1 1 2 3 4 7))
(test-assert "all families satisfy their identities and rounding rules"
  (grid-correct? small-ns small-ds))
(test-assert "inexact integers preserve the same identities and exactness"
  (grid-correct? (map inexact small-ns) (map inexact small-ds)))

(define big (expt 2 150))
(test-equal "large exact round tie stays even"
  (list (/ big 2) 1) (pair-result round/ (+ big 1) 2))
(test-equal "large exact balanced tie adjusts the quotient"
  (list (+ (/ big 2) 1) -1) (pair-result balanced/ (+ big 1) 2))
(test-assert "large signed operands satisfy every family's rules"
  (grid-correct? (list big (+ big 1) (+ big 3) (- big) (- (- big) 1))
                 (list 2 -2 7 -7 (- big 1) (- 1 big))))

(test-end "division")
