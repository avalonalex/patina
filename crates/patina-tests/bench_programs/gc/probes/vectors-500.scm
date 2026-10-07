;; 500 vectors of 100,000 elements, each dropped at once (#606): a trigger
;; that counts bytes keeps the peak near a few vectors; one that counts
;; objects never collects.
(import (scheme base) (scheme write))
(define (loop n acc)
  (if (= n 0) acc (loop (- n 1) (+ acc (vector-length (make-vector 100000 n))))))
(display (loop 500 0))
(newline)
