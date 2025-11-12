;; Test that errors don't stop execution
(import (chibi test))

(test-begin "error-recovery")

;; This should pass
(test 5 (+ 2 3))

;; This will cause an error (named let not implemented)
(test 10 (let loop ((x 10)) x))

;; This should still run and pass
(test 7 (+ 3 4))

;; This will cause another error (undefined variable)
(test 42 undefined-variable)

;; This should still run and pass
(test 15 (* 3 5))

(test-end)
