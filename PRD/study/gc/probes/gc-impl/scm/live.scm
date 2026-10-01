(import (scheme base) (scheme write) (patina debug))
; build a 2M live list while churning
(define (build n acc) (if (= n 0) acc (build (- n 1) (cons (vector n n) acc))))
(define keep (build 2000000 '()))
(display (gc-stats)) (newline)
