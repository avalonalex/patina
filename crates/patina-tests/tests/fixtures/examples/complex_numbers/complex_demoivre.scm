;;; De Moivre's Formula Test
;;; (cos θ + i sin θ)ⁿ = cos(nθ) + i sin(nθ)
;;;
;;; We'll test special cases where we know exact values

;; Test: (cos(π/4) + i sin(π/4))² should equal (cos(π/2) + i sin(π/2)) = i
;; cos(π/4) ≈ 0.7071, sin(π/4) ≈ 0.7071
;; So we're computing (0.7071 + 0.7071i)²

(define z 0.7071+0.7071i)
(* z z)  ; Should be approximately 0+i

;; Test: i⁴ = 1 (since i = e^(iπ/2))
;; This demonstrates (e^(iπ/2))⁴ = e^(i2π) = 1
(* (* +i +i) (* +i +i))

;; Test: Powers of (1+i)/√2 ≈ 0.7071(1+i)
;; This is e^(iπ/4), so its 4th power should be e^(iπ) = -1
(define unit-45 0.7071+0.7071i)
(define square (* unit-45 unit-45))
(define fourth (* square square))
fourth  ; Should be approximately -1

;; Verify: (1+i)² = 2i
(* 1+1i 1+1i)

;; Verify: (1-i)² = -2i
(* 1-1i 1-1i)

;; Verify: (1+i)(1-i) = 2 (complex conjugate pairs)
(* 1+1i 1-1i)
