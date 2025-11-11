;;; Roots of Unity - Complex numbers that satisfy z^n = 1
;;; The n-th roots of unity are: e^(2πik/n) for k = 0, 1, ..., n-1
;;; In rectangular form: cos(2πk/n) + i·sin(2πk/n)

;; Compute approximate square roots of unity
;; z² = 1  =>  z = ±1
(define root1 1+0i)
(define root2 -1+0i)

;; Verify they are square roots of 1
(* root1 root1)  ; Should be 1
(* root2 root2)  ; Should be 1

;; Compute cube roots of unity
;; z³ = 1  =>  z = 1, (-1±√3i)/2
(define cube-root1 1+0i)
(define cube-root2 -0.5+0.866i)  ; e^(2πi/3) ≈ -1/2 + √3/2 i
(define cube-root3 -0.5-0.866i)  ; e^(4πi/3) ≈ -1/2 - √3/2 i

;; Verify: multiply root by itself 3 times
(* cube-root2 (* cube-root2 cube-root2))  ; Should be close to 1
(* cube-root3 (* cube-root3 cube-root3))  ; Should be close to 1

;; Property: sum of n-th roots of unity is always 0
(+ cube-root1 cube-root2 cube-root3)  ; Should be close to 0
