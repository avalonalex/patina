(import (scheme base) (scheme write))
(let loop ((i 0))
  (when (< i 500) (make-vector 100000 0) (loop (+ i 1))))
(write 'done) (newline)
