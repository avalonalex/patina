;;; Julia Set Iterator
;;; Similar to Mandelbrot, but fixes c and varies starting point
;;; z_n+1 = z_n² + c

(define (julia-iterate z c iterations)
  (define (iter current count)
    (if (= count 0)
        current
        (iter (+ (* current current) c) (- count 1))))
  (iter z iterations))

;; Test with c = -0.7+0.27i (a classic Julia set parameter)
(julia-iterate 0+0i -0.7+0.27i 5)
(julia-iterate 0.5+0.5i -0.7+0.27i 3)
(julia-iterate 1+1i -0.7+0.27i 2)

;; Watch the iteration diverge
(julia-iterate 2+0i 0+0i 1)  ; 4
(julia-iterate 2+0i 0+0i 2)  ; 16
(julia-iterate 2+0i 0+0i 3)  ; 256
