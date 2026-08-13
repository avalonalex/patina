;; ASCII Mandelbrot set in portable R7RS-small Scheme.
;;
;; Renders the Mandelbrot set as text using escape-time iteration over
;; inexact (floating-point) arithmetic. Points inside the set print as
;; spaces; points outside are shaded by how quickly they escape.
;;
;; Run:
;;   ./target/release/patina examples/mandelbrot.scm

(import (scheme base)
        (scheme write))

(define columns 78)
(define rows 30)
(define max-iterations 96)

;; The viewport in the complex plane.
(define real-min -2.1)
(define real-max 0.7)
(define imag-min -1.2)
(define imag-max 1.2)

;; Iterate z <- z^2 + c from z = 0. Returns the number of iterations
;; before |z| exceeds 2, or #f if c appears to be in the set.
(define (escape-time real imag)
  (let loop ((zr 0.0) (zi 0.0) (iteration 0))
    (cond ((= iteration max-iterations) #f)
          ((> (+ (* zr zr) (* zi zi)) 4.0) iteration)
          (else
           (loop (+ (- (* zr zr) (* zi zi)) real)
                 (+ (* 2.0 zr zi) imag)
                 (+ iteration 1))))))

(define shades ".,:;-=+*#%@")

(define (shade iterations)
  (if iterations
      (string-ref shades
                  (remainder iterations (string-length shades)))
      #\space))

(do ((row 0 (+ row 1)))
    ((= row rows))
  (let ((imag (+ imag-min
                 (* (- imag-max imag-min)
                    (/ (inexact row) (- rows 1))))))
    (do ((col 0 (+ col 1)))
        ((= col columns))
      (let ((real (+ real-min
                     (* (- real-max real-min)
                        (/ (inexact col) (- columns 1))))))
        (display (shade (escape-time real imag)))))
    (newline)))
