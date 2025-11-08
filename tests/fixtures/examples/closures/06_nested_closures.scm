;; Nested and Recursive Closures
;; Advanced closure patterns

;; Example 1: Closure returning closure returning closure
(define make-adder-factory
  (lambda ()
    (lambda (x)
      (lambda (y)
        (lambda (z)
          (+ x (+ y z)))))))

(define factory (make-adder-factory))
(define add10 (factory 10))
(define add10-20 (add10 20))
(add10-20 5) ; => 35 (10 + 20 + 5)

;; Example 2: Closures sharing mutable state
(define make-shared-counter
  (lambda ()
    (let ((count 0))
      (lambda (operation)
        (if (eq? operation 'inc)
            (lambda ()
              (set! count (+ count 1))
              count)
            (if (eq? operation 'dec)
                (lambda ()
                  (set! count (- count 1))
                  count)
                (lambda () count)))))))

(define shared (make-shared-counter))
(define inc (shared 'inc))
(define dec (shared 'dec))
(define get (shared 'get))

(inc)  ; => 1
(inc)  ; => 2
(dec)  ; => 1
(get)  ; => 1 (same state!)

;; Example 3: Y-combinator style factorial (recursive closure)
(define make-factorial
  (lambda ()
    (lambda (n)
      (if (= n 0)
          1
          (* n ((make-factorial) (- n 1)))))))

(define fact (make-factorial))
(fact 5) ; => 120
(fact 3) ; => 6

;; Example 4: Mutually recursive closures
(define make-even-odd
  (lambda ()
    (letrec ((is-even?
               (lambda (n)
                 (if (= n 0)
                     #t
                     (is-odd? (- n 1)))))
             (is-odd?
               (lambda (n)
                 (if (= n 0)
                     #f
                     (is-even? (- n 1))))))
      (lambda (which n)
        (if (eq? which 'even?)
            (is-even? n)
            (is-odd? n))))))

(define checker (make-even-odd))
(checker 'even? 4) ; => #t
(checker 'odd? 4)  ; => #f
(checker 'even? 7) ; => #f
(checker 'odd? 7)  ; => #t

;; Expected output: 35, 1, 2, 1, 1, 120, 6, #t, #f, #f, #t
