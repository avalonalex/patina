(import (scheme base) (scheme cxr) (scheme inexact)
        (prefix (slib daylight) d:) (prefix (slib color-space) c:) (patina compat smoke))
(define (near? x y) (< (abs (- x y)) 1e-8))
(define (triple-near? xs ys) (and (near? (car xs) (car ys))
  (near? (cadr xs) (cadr ys)) (near? (caddr xs) (caddr ys))))
(check-equal "solar time preserves the elapsed hours on a fixed day" #t
  (near? 6 (- (d:solar-hour 172 18) (d:solar-hour 172 12))))
(check-equal "declination is zero at the model's spring equinox" #t
  (near? 0 (d:solar-declination 81)))
(check-equal "opposite quarter-years have opposite declinations" #t
  (near? (d:solar-declination 173) (- (d:solar-declination 357))))
(check-equal "equinox noon zenith angle equals northern latitude" #t
  (near? 40 (car (d:solar-polar 0 40 12))))
(check-equal "equinox sunrise and sunset lie on the equatorial horizon" #t
  (and (near? 90 (car (d:solar-polar 0 0 6)))
       (near? 90 (car (d:solar-polar 0 0 18)))))
(check-equal "sunlight has 41 positive spectral samples" #t
  (let ((s (d:sunlight-spectrum 2 30)))
    (and (= 41 (vector-length s))
         (let loop ((i 0)) (or (= i 41) (and (> (vector-ref s i) 0) (loop (+ i 1))))))))
(check-equal "sunlight chromaticity integrates its spectrum" #t
  (let ((a (d:sunlight-chromaticity 2 30))
        (b (c:spectrum->chromaticity (d:sunlight-spectrum 2 30) 380e-9 780e-9)))
    (and (near? (car a) (car b)) (near? (cadr a) (cadr b)))))
(check-equal "increased turbidity attenuates direct sunlight" #t
  (< (vector-ref (d:sunlight-spectrum 5 30) 20) (vector-ref (d:sunlight-spectrum 2 30) 20)))
(check-equal "zenith chromaticities match the polynomial's zero-angle coefficients" #t
  (let ((z (d:zenith-xyY 2 0)))
    (and (near? 0.26673 (car z)) (near? 0.27718 (cadr z)) (> (caddr z) 0))))
(check-equal "overcast zenith reproduces the zenith model" #t
  (triple-near? (d:zenith-xyY 25 30) ((d:overcast-sky-color-xyY 25 30) 0)))
(check-equal "overcast horizon has one-third zenith luminance" #t
  (near? (/ (caddr (d:zenith-xyY 25 30)) 3)
         (caddr ((d:overcast-sky-color-xyY 25 30) 90))))
(check-equal "overcast sky is independent of azimuth" #t
  (triple-near? ((d:overcast-sky-color-xyY 25 30) 45 0)
               ((d:overcast-sky-color-xyY 25 30) 45 180)))
(check-equal "clear sky normalization reproduces zenith xyY" #t
  (triple-near? (d:zenith-xyY 2 30) ((d:clear-sky-color-xyY 2 30 0) 0 0)))
(check-equal "clear sky is invariant under rotation of both azimuths" #t
  (triple-near? ((d:clear-sky-color-xyY 2 30 20) 45 60)
               ((d:clear-sky-color-xyY 2 30 50) 45 90)))
(check-equal "sky dispatcher selects clear then overcast with turbidity" #t
  (and (triple-near? ((d:sky-color-xyY 2 30 20) 45 60)
                     ((d:clear-sky-color-xyY 2 30 20) 45 60))
       (triple-near? ((d:sky-color-xyY 25 30 20) 45 60)
                     ((d:overcast-sky-color-xyY 25 30) 45 60))))
(smoke-finish)
