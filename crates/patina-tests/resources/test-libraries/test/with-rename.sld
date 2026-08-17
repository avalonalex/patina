;; Test library with renamed exports
(define-library (test with-rename)
  ;; `define` is a binding, imported like any other (stage 2).
  (import (scheme base))
  (export square (rename internal-cube cube))

  (begin
    (define (square x)
      (* x x))

    (define (internal-cube x)
      (* x x x))))
