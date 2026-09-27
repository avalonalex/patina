(import (scheme base) (scheme inexact) (srfi 27)
        (prefix (slib random-inexact) r:) (patina compat smoke))
(define (near? x y) (< (abs (- x y)) 1e-10))
(define (norm2 v) (let loop ((i 0) (sum 0))
  (if (= i (vector-length v)) sum
      (loop (+ i 1) (+ sum (* (vector-ref v i) (vector-ref v i)))))))
;; Supply exact uniform samples to test the transformations, not a host PRNG's
;; sequence. Equal samples within each Box-Muller pair allow either let
;; initializer order. Restore the shared imported binding even if a call raises.
(define (with-samples samples thunk)
  (let ((saved random-real) (pending samples))
    (dynamic-wind
      (lambda () (set! random-real
        (lambda () (if (null? pending) (error "sample stream exhausted")
          (let ((x (car pending))) (set! pending (cdr pending)) x)))))
      thunk (lambda () (set! random-real saved)))))
(check-equal "uniform returns the supplied uniform sample" 0.25
  (with-samples '(0.25) r:random:uniform))
(check-equal "exponential applies negative logarithm" #t
  (near? (log 4) (with-samples '(0.25) r:random:exp)))
(check-equal "normal samples use the Box-Muller cosine component" #t
  (near? (- (sqrt (* 2 (log 2))))
         (with-samples '(0.5 0.5) r:random:normal)))
(check-equal "even vectors use sine and cosine components and return squared norm" #t
  (let ((v (make-vector 2)))
    (let ((sum (with-samples '(0.25 0.25) (lambda () (r:random:normal-vector! v)))))
      (and (near? (sqrt (* 2 (log 4))) (vector-ref v 0))
           (near? 0 (vector-ref v 1)) (near? sum (norm2 v))))))
(check-equal "odd vectors consume a final pair without writing past the end" #t
  (let ((v (make-vector 3)))
    (with-samples '(0.25 0.25 0.25 0.25) (lambda () (r:random:normal-vector! v)))
    (and (near? 0 (vector-ref v 0))
         (near? (sqrt (* 2 (log 4))) (vector-ref v 1))
         (near? 0 (vector-ref v 2)))))
(check-equal "empty normal vectors consume no samples and have zero norm" 0
  (with-samples '() (lambda () (r:random:normal-vector! (vector)))))
(check-equal "hollow sphere normalizes the vector to unit length" #t
  (let ((v (make-vector 3)))
    (with-samples '(0.25 0.25 0.25 0.25) (lambda () (r:random:hollow-sphere! v)))
    (near? 1 (norm2 v))))
(check-equal "one-dimensional sphere lies at a unit endpoint" #t
  (let ((v (make-vector 1)))
    (with-samples '(0.5 0.5) (lambda () (r:random:hollow-sphere! v)))
    (near? -1 (vector-ref v 0))))
(check-equal "solid sphere scales radius by the dimension" #t
  (let ((v (make-vector 2)))
    (with-samples '(0.25 0.25 0.25) (lambda () (r:random:solid-sphere! v)))
    (near? 0.25 (norm2 v))))
(check-equal "solid sphere returns the documented sum of squares" #t
  (let ((v (make-vector 2)))
    (let ((sum (with-samples '(0.25 0.25 0.25) (lambda () (r:random:solid-sphere! v)))))
      (near? sum (norm2 v)))))
;; Compare each host with its own saved state; seeded streams need not agree
;; across implementations. Restore the source after these checks as well.
(define saved-state (random-source-state-ref default-random-source))
(check-equal "optional state repeats uniform sampling" #t
  (= (r:random:uniform saved-state) (r:random:uniform saved-state)))
(check-equal "optional state repeats exponential and normal sampling" #t
  (and (= (r:random:exp saved-state) (r:random:exp saved-state))
       (= (r:random:normal saved-state) (r:random:normal saved-state))))
(check-equal "optional state repeats vector sampling" #t
  (let ((a (make-vector 3)) (b (make-vector 3)))
    (r:random:solid-sphere! a saved-state)
    (r:random:solid-sphere! b saved-state)
    (equal? a b)))
(random-source-state-set! default-random-source saved-state)
(smoke-finish)
