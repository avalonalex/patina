(import (scheme base) (scheme ephemeron) (patina debug) (scheme time) (scheme write) (scheme process-context))
(define n (string->number (cadr (command-line))))
(define order (caddr (command-line)))
(define k1 (list 0))
;; chain: e_i holds key k_i and datum k_{i+1}; only k1 is strongly reachable.
(define ephs
  (let loop ((i 0) (k k1) (acc '()))
    (if (= i n) acc
        (let ((k2 (list i)))
          (loop (+ i 1) k2 (cons (make-ephemeron k k2) acc))))))
(define ephs2 (if (string=? order "fwd") (reverse ephs) ephs))
(set! ephs #f)
(gc)
(define t0 (current-jiffy))
(gc)
(define t1 (current-jiffy))
(define broken (let loop ((l ephs2) (c 0)) (if (null? l) c (loop (cdr l) (if (ephemeron-broken? (car l)) (+ c 1) c)))))
(write (list 'n n order 'ms (/ (* 1000.0 (- t1 t0)) (jiffies-per-second)) 'broken broken)) (newline)
