(import (scheme base) (scheme write)) (define (bl n) (if (= n 0) '() (cons n (bl (- n 1))))) (display (length (bl 2000000))) (newline)
