;;; Mandelbrot Set - Test if a point is in the set
;;; The Mandelbrot set is defined by iterating z = z² + c
;;; A point c is in the set if |z| doesn't diverge to infinity

;; Helper: compute magnitude squared (faster than sqrt for comparison)
(define (magnitude-squared z)
  (+ (* (real-part z) (real-part z))
     (* (imag-part z) (imag-part z))))

;; Test if a complex point c is in the Mandelbrot set
;; Iterate z = z² + c up to max-iter times
;; If |z| > 2, it escapes (not in set)
(define (in-mandelbrot? c max-iter)
  (define (iter z count)
    (cond
      ((> count max-iter) #t)  ; Didn't escape, probably in set
      ((> (magnitude-squared z) 4.0) #f)  ; Escaped (|z| > 2)
      (else (iter (+ (* z z) c) (+ count 1)))))
  (iter 0+0i 0))

;; Test some known points
(in-mandelbrot? 0+0i 50)        ; Origin - definitely in set
(in-mandelbrot? 0.25+0i 50)     ; On real axis - in set
(in-mandelbrot? 1+1i 50)        ; Outside - not in set
(in-mandelbrot? -0.5+0.5i 50)   ; Near edge - in set
