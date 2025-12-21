;;; PRIMES -- Sieve of Eratosthenes.
;;; From ecraven/r7rs-benchmarks.
;;;
;;; Tests list operations, filtering, recursion.

(define (interval-list m n)
  (if (> m n)
      '()
      (cons m (interval-list (+ 1 m) n))))

(define (sieve l)
  (letrec ((remove-multiples
            (lambda (n l)
              (if (null? l)
                  '()
                  (if (= (remainder (car l) n) 0)
                      (remove-multiples n (cdr l))
                      (cons (car l)
                            (remove-multiples n (cdr l))))))))
    (if (null? l)
        '()
        (cons (car l)
              (sieve (remove-multiples (car l) (cdr l)))))))

(define (primes<= n)
  (sieve (interval-list 2 n)))

;; Standard benchmark: (primes<= 1000) for 10000 iterations
;; Smaller for quick testing: (primes<= 100) for 100 iterations
