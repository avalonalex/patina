(import (scheme base) (scheme write))
(define (deep d) (if (= d 0) 0 (+ 1 (deep (- d 1)))))
(display (deep 10000000))
