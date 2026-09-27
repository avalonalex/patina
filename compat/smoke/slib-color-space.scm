(import (scheme base) (scheme inexact) (prefix (slib color-space) c:)
        (patina compat smoke))
(define (near? xs ys tolerance)
  (and (list? ys) (= (length xs) (length ys))
       (let loop ((xs xs) (ys ys))
         (or (null? xs) (and (< (abs (- (car xs) (car ys))) tolerance)
                            (loop (cdr xs) (cdr ys)))))))
(define (observe thunk) (guard (ex (else 'unexpected-error)) (thunk)))
(check-equal "linear transforms contract rows against the input" '(14 32)
  (c:color:linear-transform '((1 2 3) (4 5 6)) '(1 2 3)))
(check-equal "packed RGB retains leading zero channels" '(0 128 255) (c:xRGB->sRGB #x0080ff))
(check-equal "RGB packing uses channel order" #x123456 (c:sRGB->xRGB '(18 52 86)))
(check-equal "linear red has the documented XYZ matrix column" #t
  (near? '(0.412453 0.212671 0.019334) (c:RGB709->CIEXYZ '(1 0 0)) 1e-9))
(check-equal "RGB709 matrix inverse round trip" #t
  (near? '(0.2 0.4 0.8) (c:CIEXYZ->RGB709 (c:RGB709->CIEXYZ '(0.2 0.4 0.8))) 2e-6))
(check-equal "sRGB gamma and integer rounding round trip dark and bright channels" '(1 128 255)
  (c:CIEXYZ->sRGB (c:sRGB->CIEXYZ '(1 128 255))))
(check-equal "XYZ conversion clips out-of-gamut RGB channels" '(0 255 0)
  (c:CIEXYZ->sRGB (c:RGB709->CIEXYZ '(-1 2 -1))))
(check-equal "packed RGB XYZ round trip" #x804020 (c:CIEXYZ->xRGB (c:xRGB->CIEXYZ #x804020)))
(check-equal "extended RGB adds its precision-specific offset" '(384 640 894)
  (c:sRGB->e-sRGB 10 '(0 128 255)))
(check-equal "extended RGB decodes to ordinary channel values" '(0 128 255)
  (c:e-sRGB->sRGB 10 '(384 640 894)))
(check-equal "changing extended precision rescales encoded channels" '(1536 2560 3576)
  (c:e-sRGB->e-sRGB 10 '(384 640 894) 12))
(check-equal "extended RGB XYZ round trip retains encoded channels" '(20000 35000 50000)
  (c:CIEXYZ->e-sRGB 16 (c:e-sRGB->CIEXYZ 16 '(20000 35000 50000))))
(check-equal "reference white maps to neutral Lab" #t
  (near? '(100 0 0) (c:CIEXYZ->L*a*b* c:CIEXYZ:D65) 1e-9))
(check-equal "black maps to zero Lab and Luv" #t
  (and (near? '(0 0 0) (c:CIEXYZ->L*a*b* '(0 0 0)) 1e-9)
       (near? '(0 0 0) (c:CIEXYZ->L*u*v* '(0 0 0)) 1e-9)))
(check-equal "Lab round trip covers the low-light linear branch" #t
  (near? '(0.001 0.002 0.003) (c:L*a*b*->CIEXYZ (c:CIEXYZ->L*a*b* '(0.001 0.002 0.003))) 1e-7))
(check-equal "Lab round trip honors explicit white points" #t
  (near? '(0.2 0.3 0.4)
    (c:L*a*b*->CIEXYZ (c:CIEXYZ->L*a*b* '(0.2 0.3 0.4) c:CIEXYZ:D50) c:CIEXYZ:D50) 1e-9))
(check-equal "Luv round trip honors explicit white points" #t
  (near? '(0.2 0.3 0.4)
    (c:L*u*v*->CIEXYZ (c:CIEXYZ->L*u*v* '(0.2 0.3 0.4) c:CIEXYZ:E) c:CIEXYZ:E) 1e-9))
(check-equal "zero Luv lightness gives black without division by zero" #t
  (near? '(0 0 0) (c:L*u*v*->CIEXYZ '(0 0 0)) 1e-9))
(check-equal "polar Lab normalizes negative hue into degrees" #t
  (near? '(50 10 270) (c:L*a*b*->L*C*h '(50 0 -10)) 1e-9))
(check-equal "polar Lab inverse uses degrees" #t
  (near? '(50 0 10) (c:L*C*h->L*a*b* '(50 10 90)) 1e-9))
(check-equal "Euclidean Lab difference follows the 3-4-5 triangle" #t
  (< (abs (- 5 (c:L*a*b*:DE* '(50 0 0) '(50 3 4)))) 1e-9))
(check-equal "CIE94 identity has zero difference" #t
  (< (abs (c:L*a*b*:DE*94 '(50 10 20) '(50 10 20))) 1e-9))
(check-equal "CMC accepts the documented single numeric lightness factor" #t
  (observe (lambda ()
    (= (c:CMC-DE '(50 10 20) '(52 10 20) 2)
       (c:CMC-DE '(50 10 20) '(52 10 20) '(2 1))))))
(check-equal "CIE94 divides lightness difference by its parametric factor" #t
  (< (abs (- 2 (c:L*a*b*:DE*94 '(50 0 0) '(54 0 0) '(2 1 1)))) 1e-9))
(check-equal "CIE94 chroma scaling is squared in the distance" #t
  (< (abs (- (/ 20 13) (c:L*a*b*:DE*94 '(50 50 0) '(50 45 0)))) 1e-9))
(check-equal "CIE94 hue scaling is squared in the distance" #t
  (< (abs (- (/ (sqrt 50) 1.075) (c:L*a*b*:DE*94 '(50 3 4) '(50 -4 3)))) 1e-9))
(check-equal "CIE94 list factors equal separate factors" #t
  (observe (lambda ()
    (= (c:L*a*b*:DE*94 '(50 3 4) '(60 3 4) '(1 1 1))
       (c:L*a*b*:DE*94 '(50 3 4) '(60 3 4) 1 1 1)))))
(check-equal "chromaticity separates intensity from ratios" '(1/6 1/3)
  (c:XYZ->chromaticity '(1 2 3)))
(check-equal "chromaticity reconstructs a normalized XYZ triple" '(1/4 1/2 1/4)
  (c:chromaticity->CIEXYZ 1/4 1/2))
(check-equal "whitepoint has unit luminance" '(1/2 1 1/2)
  (c:chromaticity->whitepoint 1/4 1/2))
(check-equal "xyY round trip preserves luminance" '(1 2 3) (c:xyY->XYZ (c:XYZ->xyY '(1 2 3))))
(check-equal "black xyY conversion handles zero denominators" '((0 0 0) (0 0 0))
  (list (c:XYZ->xyY '(0 0 0)) (c:xyY->XYZ '(0 0 0))))
(check-equal "normalization clips chromaticities and scales luminance" '((0 1/2 1/2) (1/2 1/2 1))
  (c:xyY:normalize-colors '((-1 1/2 2) (1 1 4))))
(check-equal "wavelength lookup interpolates neighboring CIE table rows" #t
  (near? (map (lambda (a b) (/ (+ a b) 2)) (c:wavelength->XYZ 550e-9) (c:wavelength->XYZ 555e-9))
         (c:wavelength->XYZ 552.5e-9) 1e-9))
(check-equal "775nm lookup does not read beyond the CIE table" #t
  (near? '(0.0001 0.0000 0.0000) (observe (lambda () (c:wavelength->XYZ 775e-9))) 1e-9))
(check-equal "wavelength interpolation reaches the documented 780nm endpoint" #t
  (and (near? '(0.00005 0 0) (observe (lambda () (c:wavelength->XYZ 777.5e-9))) 1e-9)
       (near? '(0 0 0) (observe (lambda () (c:wavelength->XYZ 780e-9))) 1e-9)))
(check-equal "reversed nonconstant spectra preserve wavelength association" #t
  (let ((spectrum (let loop ((i 0) (xs '()))
                    (if (= i 81) xs (loop (+ i 1) (cons (+ i 1) xs))))))
    (near? (c:spectrum->XYZ spectrum 380e-9 780e-9)
           (observe (lambda () (c:spectrum->XYZ (reverse spectrum) 780e-9 380e-9))) 1e-9)))
(check-error "out-of-range wavelengths are rejected" (c:wavelength->XYZ 300e-9))
(check-equal "constant spectra agree between procedure and sampled forms" #t
  (near? (c:spectrum->XYZ (lambda (w) 1)) (c:spectrum->XYZ '#(1 1) 380e-9 780e-9) 1e-9))
(check-equal "reversing sampled spectrum endpoints preserves the integral" #t
  (near? (c:spectrum->XYZ (make-vector 81 1) 380e-9 780e-9)
         (observe (lambda () (c:spectrum->XYZ (make-vector 81 1) 780e-9 380e-9))) 1e-9))
(check-equal "illuminant mapping visits every wavelength and leaves input intact" '(107 107 #t 2)
  (let* ((input (make-vector 107 2)) (calls 0)
         (out (c:illuminant-map (lambda (w x) (set! calls (+ calls 1)) (* 3 x)) input)))
    (list calls (vector-length out)
          (equal? out (make-vector 107 6)) (vector-ref input 0))))
(check-equal "blackbody spectrum scales with requested wavelength span" #t
  (let ((a ((c:blackbody-spectrum 6500) 550e-9))
        (b ((c:blackbody-spectrum 6500 2e-9) 550e-9)))
    (and (> a 0) (< (abs (- 2 (/ b a))) 1e-9))))
(check-equal "temperature chromaticity agrees with integrated XYZ" #t
  (near? (c:XYZ->chromaticity (c:temperature->XYZ 6500))
         (c:temperature->chromaticity 6500) 1e-9))
(smoke-finish)
