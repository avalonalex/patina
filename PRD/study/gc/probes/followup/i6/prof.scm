(import (scheme base) (patina debug))
(length (make-list 2000000 0))
(let loop ((i 0)) (if (< i 1500) (begin (gc) (cons 1 2) (loop (+ i 1)))))
