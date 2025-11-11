;; Simple test library for validating .sld loading
(define-library (test simple)
  (export double triple add-one)

  (begin
    (define (double x)
      (* x 2))

    (define (triple x)
      (* x 3))

    (define (add-one x)
      (+ x 1))))
