;; 16-byte and 48-byte objects with interleaved lifetimes, the shape of
;; Wingo's livelock: a long-lived ring keeps every other object of a stream
;; that alternates pairs and 4-element vectors, so the survivors of each
;; collection sit between the dead.
(import (scheme base) (scheme write) (scheme process-context))
(define (arg k default) (if (> (length (command-line)) k) (string->number (list-ref (command-line) k)) default))
(define ring (make-vector 200000 #f))
(define (step i)
  (let ((small (cons i i)) (big (vector i i i i)))
    (vector-set! ring (modulo (* 2 i) 200000) (if (even? i) small big))
    (if (odd? i) small big)))
(let loop ((i 0) (n (arg 1 6000000)))
  (if (< i n) (begin (step i) (loop (+ i 1) n))))
(display (vector-length ring))
(newline)
