;; 100,000 file ports opened and dropped, never closed (#607): run under
;; ulimit -n 1024, so it finishes only if collections close the dead ones.
(import (scheme base) (scheme write) (scheme file) (scheme process-context))
(define path (list-ref (command-line) 1))
(call-with-output-file path (lambda (p) (write 'x p)))
(let loop ((i 0))
  (if (< i 100000)
      (begin (open-input-file path) (loop (+ i 1)))))
(display "ok")
(newline)
