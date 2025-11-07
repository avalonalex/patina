;;; Complex Fibonacci Sequence
;;; Fibonacci with complex numbers: F(n) = F(n-1) + F(n-2)
;;; Starting with complex initial values

(define (complex-fib n a b)
  (if (= n 0)
      a
      (if (= n 1)
          b
          (complex-fib (- n 1) b (+ a b)))))

;; Traditional Fibonacci with real numbers
(complex-fib 0 0 1)  ; 0
(complex-fib 1 0 1)  ; 1
(complex-fib 5 0 1)  ; 5
(complex-fib 10 0 1) ; 55

;; Complex Fibonacci starting with imaginary seeds
(complex-fib 0 0+0i 0+1i)  ; 0i
(complex-fib 1 0+0i 0+1i)  ; i
(complex-fib 2 0+0i 0+1i)  ; i
(complex-fib 3 0+0i 0+1i)  ; 2i
(complex-fib 4 0+0i 0+1i)  ; 3i
(complex-fib 5 0+0i 0+1i)  ; 5i

;; Complex Fibonacci with mixed seeds
(complex-fib 0 1+1i 1-1i)  ; 1+i
(complex-fib 1 1+1i 1-1i)  ; 1-i
(complex-fib 2 1+1i 1-1i)  ; 2+0i = 2
(complex-fib 3 1+1i 1-1i)  ; 3-i
(complex-fib 4 1+1i 1-1i)  ; 5-i
(complex-fib 5 1+1i 1-1i)  ; 8-2i
