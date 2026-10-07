;; Server-style: a file written and read back, each port closed, per cycle.
;; Argument 2 is the file.
(import (scheme base) (scheme write) (scheme file) (scheme process-context) (patina debug))
(include "driver.scm")
(define path (steady-arg 2))
(steady-run
 (lambda (i)
   (let ((out (open-output-file path)))
     (write i out)
     (close-port out))
   (let ((in (open-input-file path)))
     (read-char in)
     (close-port in))))
