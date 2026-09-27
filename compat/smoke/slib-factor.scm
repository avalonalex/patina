(import (scheme base) (prefix (slib factor) f:) (patina compat smoke))
(define (same-factors? actual expected)
  (define (delete-one x xs)
    (cond ((null? xs) #f) ((= x (car xs)) (cdr xs))
          (else (let ((rest (delete-one x (cdr xs)))) (and rest (cons (car xs) rest))))))
  (cond ((null? actual) (null? expected))
        (else (let ((rest (delete-one (car actual) expected)))
                (and rest (same-factors? (cdr actual) rest))))))
(check-equal "small primality cases include zero, one and composites"
  '(#f #f #t #t #f #t #f #t #f)
  (map f:prime? '(0 1 2 3 4 7 9 97 121)))
;; A true prime cannot be a probabilistic false positive. Composite cases above
;; are caught by the package's deterministic small-prime checks.
(check-equal "the large-prime path accepts a known prime" #t (f:prime? 1000003))
(check-equal "prime trial counts can be scoped and restored" '(4 #t)
  (let ((saved (f:prime:trials)))
    (let ((inside (parameterize ((f:prime:trials 4)) (f:prime:trials))))
      (list inside (= saved (f:prime:trials))))))
(check-equal "Jacobi symbols cover residues, nonresidues and common factors" '(1 -1 0 1)
  (list (f:jacobi-symbol 4 7) (f:jacobi-symbol 3 7)
        (f:jacobi-symbol 7 7) (f:jacobi-symbol 2 15)))
(check-equal "prime enumeration above a bound selects the nearest primes" #t
  (same-factors? (f:primes> 10 4) '(11 13 17 19)))
(check-equal "prime enumeration below a bound selects the nearest primes" #t
  (same-factors? (f:primes< 10 3) '(3 5 7)))
(check-equal "enumerating below two returns no primes" '() (f:primes< 2 3))
(check-equal "enumeration below a bound can return fewer primes than requested" #t
  (same-factors? (f:primes< 5 8) '(2 3)))
(check-equal "requesting zero primes returns empty lists" '(() ())
  (list (f:primes< 10 0) (f:primes> 10 0)))
(check-equal "requesting one prime returns exactly the nearest one" '((7) (11))
  (list (f:primes< 10 1) (f:primes> 10 1)))
(check-equal "factorization keeps the documented special values" '((-1) (0) (1))
  (map f:factor '(-1 0 1)))
(check-equal "factorization preserves repeated powers of two" #t
  (same-factors? (f:factor 64) '(2 2 2 2 2 2)))
(check-equal "factorization preserves odd multiplicities" #t
  (same-factors? (f:factor 225) '(3 3 5 5)))
(check-equal "negative integers carry one sign factor" #t
  (same-factors? (f:factor -84) '(-1 2 2 3 7)))
(check-equal "prime inputs remain one factor" '(97) (f:factor 97))
(check-equal "mixed factors reconstruct the input" #t
  (let ((factors (f:factor 2310)))
    (and (= 2310 (apply * factors)) (same-factors? factors '(2 3 5 7 11)))))
(smoke-finish)
