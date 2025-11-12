;; Test that (chibi test) works

(import (scheme base) (chibi test))

(test-begin "Demo")

(test 6 (+ 1 2 3))
(test #t (boolean? #t))
(test '(1 2 3) (list 1 2 3))

;; This one should fail
;; (test 999 (+ 1 1))

(test-end)
