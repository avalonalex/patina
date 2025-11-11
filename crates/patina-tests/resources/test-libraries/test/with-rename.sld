;; Test library with renamed exports
(define-library (test with-rename)
  (export square (rename internal-cube cube))

  (begin
    (define (square x)
      (* x x))

    (define (internal-cube x)
      (* x x x))))
