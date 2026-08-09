;;; Tests for SRFI-156 drawn from the SRFI itself
(define-library (srfi 156 test)
  (export run-tests)
  (import (scheme base)
	  (srfi 156)
	  (chibi test))
  (begin
    (define (run-tests)
      (test-begin "srfi-156")

      ;; Infix relations
      (test #t (is 1 < 10))
      (test #f (is 1 > 10))

      ;; Short-hand lambda expressions
      (test #t ((is _ < 10) 1))
      (test #f ((is _ > 10) 1))
      (test #t ((is 1 < _) 10))
      (test #f ((is 1 > _) 10))
      (test #t ((is _ < _) 1 10))
      (test #f ((is _ > _) 1 10))

      ;; Negation
      (test #f (isnt 1 < 10))
      (test #t (isnt 1 > 10))
      (test #f ((isnt _ < 10) 1))
      (test #t ((isnt _ > 10) 1))
      (test #f ((isnt 1 < _) 10))
      (test #t ((isnt 1 > _) 10))
      (test #f ((isnt _ < _) 1 10))
      (test #t ((isnt _ > _) 1 10))

      ;; Fewer arguments:
      (test #f (isnt #\x char?))
      (test #t (isnt #\x string?))

      ;; More arguments
      (test #t (is 1 < 2 < 3))
      (test #f (is 1 < 3 < 2))
      (test #f (is 2 < 1 < 3))
      (test #f (is 2 < 3 < 1))
      (test #f (is 3 < 1 < 2))
      (test #f (is 3 < 2 < 1))

      (test-end))))
