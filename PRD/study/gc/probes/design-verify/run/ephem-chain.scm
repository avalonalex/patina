(import (scheme base) (scheme write) (scheme time) (scheme process-context)
        (srfi 124) (patina debug))
;; A chain e_i = (make-ephemeron k_i k_(i+1)); the program holds k_0 and the
;; ephemerons, in the order given on the command line.
(define n (string->number (cadr (command-line))))
(define forward? (string=? (caddr (command-line)) "forward"))
(define keys (let loop ((i n) (acc '())) (if (< i 0) acc (loop (- i 1) (cons (list i) acc)))))
(define k0 (car keys))
(define ephs                                   ; e_(n-1) ... e_0
  (let loop ((ks keys) (acc '()))
    (if (null? (cdr ks)) acc (loop (cdr ks) (cons (make-ephemeron (car ks) (cadr ks)) acc)))))
(define held (if forward? (reverse ephs) ephs))
(set! keys #f) (set! ephs #f)
(define t0 (current-jiffy))
(gc)
(define t1 (current-jiffy))
(write (list n (if forward? 'forward 'reverse)
             (exact (round (/ (* 1000 (- t1 t0)) (jiffies-per-second)))) 'ms))
(newline)
