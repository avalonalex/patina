;; Simple test to verify test infrastructure works
(import (scheme base) (chibi test))

(test-begin "Simple Test")

(test 6 (+ 1 2 3))
(test #t (> 5 3))
(test '(1 2 3) (list 1 2 3))

(test-end)
