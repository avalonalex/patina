;;; NQUEENS -- The N-queens problem.
;;; From ecraven/r7rs-benchmarks.
;;;
;;; Tests backtracking search, list operations.

(define (nqueens n)

  (define (one-to n)
    (let loop ((i n) (l '()))
      (if (= i 0)
          l
          (loop (- i 1) (cons i l)))))

  (define (try x y z)
    (if (null? x)
        (if (null? y)
            1
            0)
        (+ (if (ok? (car x) 1 z)
               (try (append (cdr x) y) '() (cons (car x) z))
               0)
           (try (cdr x) (cons (car x) y) z))))

  (define (ok? row dist placed)
    (if (null? placed)
        #t
        (and (not (= (car placed) (+ row dist)))
             (not (= (car placed) (- row dist)))
             (ok? row (+ dist 1) (cdr placed)))))

  (try (one-to n) '() '()))

;; Standard benchmark: (nqueens 13) for 10 iterations
;; Results: nqueens(8)=92, nqueens(10)=724, nqueens(13)=73712
;; Smaller for quick testing: (nqueens 10)
