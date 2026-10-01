(define (bench-list n) (if (= n 0) '() (cons n (bench-list (- n 1))))) (define test-list (bench-list 1000)) (length (map (lambda (x) (* x 2)) test-list))
