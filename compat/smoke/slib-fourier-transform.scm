(import (scheme base) (scheme complex) (prefix (srfi 63) a:)
        (prefix (slib fourier-transform) f:) (patina compat smoke))
(define (near? expected actual)
  (and (= (length expected) (length actual))
       (let loop ((xs expected) (ys actual))
         (or (null? xs) (and (< (magnitude (- (car xs) (car ys))) 1e-9)
                            (loop (cdr xs) (cdr ys)))))))
(check-equal "an impulse has a flat slow transform" #t
  (near? '(1 1 1) (vector->list (f:sft '#(1 0 0)))))
(check-equal "constant signals have only a DC component" #t
  (near? '(8 0 0 0) (vector->list (f:fft '#(2 2 2 2)))))
(check-equal "forward transforms use the negative exponential" #t
  (near? '(0 4 0 0) (vector->list (f:fft '#(1 +i -1 -i)))))
(check-equal "slow transforms handle non-power-of-two lengths" #t
  (near? '(3 -1.5+0.8660254037844386i -1.5-0.8660254037844386i)
         (vector->list (f:sft '#(0 1 2)))))
(check-equal "slow inverse round trip includes normalization" #t
  (near? '(1 2 3) (vector->list (f:sft-1 (f:sft '#(1 2 3))))))
(check-equal "fast inverse round trip includes complex components" #t
  (near? '(1+2i 3-4i 5 6) (vector->list (f:fft-1 (f:fft '#(1+2i 3-4i 5 6))))))
(check-equal "automatic transform selects both algorithms" #t
  (and (near? '(1 2 3) (vector->list (f:dft-1 (f:dft '#(1 2 3)))))
       (near? '(1 2 3 4) (vector->list (f:dft-1 (f:dft '#(1 2 3 4)))))))
(check-equal "singleton transforms preserve the sample" #t
  (near? '(7+2i) (vector->list (f:fft '#(7+2i)))))
(check-equal "transform allocates output without changing input" '(#t #(1 2 3 4))
  (let* ((input (vector 1 2 3 4)) (output (f:fft input)))
    (list (not (eq? input output)) input)))
(check-equal "rank-two impulse transforms along both axes" '((2 2) #t)
  (let ((out (f:fft (a:list->array 2 '#() '((1 0) (0 0))))))
    (list (a:array-dimensions out) (near? '(1 1 1 1) (apply append (a:array->list out))))))
(check-equal "mixed dimensions round trip through automatic transform" #t
  (let* ((input (a:list->array 2 '#() '((1 2 3) (4 5 6))))
         (out (f:dft-1 (f:dft input '#()))))
    (near? '(1 2 3 4 5 6) (apply append (a:array->list out)))))
(check-error "fast forward rejects non-power-of-two axes" (f:fft '#(1 2 3)))
(check-error "fast inverse rejects non-power-of-two axes" (f:fft-1 '#(1 2 3)))
(smoke-finish)
