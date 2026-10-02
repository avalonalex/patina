(import (scheme base) (scheme write) (scheme time) (patina debug))
(define (now-ms) (/ (* 1000.0 (current-jiffy)) (jiffies-per-second)))
(define (gc-ms)                   ; mean wall time of 20 forced collections
  (let loop ((i 0) (total 0.0))
    (if (= i 20)
        (/ total 20)
        (let ((t0 (now-ms)))
          (gc) (cons 1 2)         ; (gc) collects at the next safe point
          (loop (+ i 1) (+ total (- (now-ms) t0)))))))
(define (stat k) (cdr (assq k (gc-stats))))
(display (list 'before (gc-ms) (stat 'pairs) (stat 'free-pairs))) (newline)
(length (make-list 2000000 0))    ; the peak, dropped at once
(display (list 'after (gc-ms) (stat 'pairs) (stat 'free-pairs))) (newline)
