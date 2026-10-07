;; ctak under a 1,000-frame non-tail stack: every capture holds the frames
;; above it as well as ctak's own.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define (ctak x y z)
  (call-with-current-continuation
   (lambda (k) (ctak-aux k x y z))))
(define (ctak-aux k x y z)
  (if (not (< y x))
      (k z)
      (call-with-current-continuation
       (lambda (k)
         (ctak-aux
          k
          (call-with-current-continuation (lambda (k) (ctak-aux k (- x 1) y z)))
          (call-with-current-continuation (lambda (k) (ctak-aux k (- y 1) z x)))
          (call-with-current-continuation (lambda (k) (ctak-aux k (- z 1) x y))))))))
(define (deep d thunk) (if (= d 0) (thunk) (+ 0 (deep (- d 1) thunk))))
(define (repeat n) (if (= n 1) (ctak 18 12 6) (begin (ctak 18 12 6) (repeat (- n 1)))))
(display (deep 1000 (lambda () (repeat (arg 1 2)))))
(newline)
