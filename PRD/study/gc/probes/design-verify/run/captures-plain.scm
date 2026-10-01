(import (scheme base) (scheme write))
;; 20,000 continuations captured 1,000 frames deep and dropped at once.
(define (at-depth d thunk) (if (= d 0) (thunk) (+ 1 (at-depth (- d 1) thunk))))
(at-depth 1000
  (lambda ()
    (let loop ((i 0))
      (when (< i 20000) (call/cc (lambda (k) k)) (loop (+ i 1))))
    0))
(write (quote done)) (newline)
