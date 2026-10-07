;; Server-style: a string port written, read back and closed, per cycle.
(import (scheme base) (scheme write) (scheme read) (scheme process-context) (patina debug))
(include "driver.scm")
(steady-run
 (lambda (i)
   (let ((out (open-output-string)))
     (write (list i "abc" 'sym (vector 1 2 3)) out)
     (let ((in (open-input-string (get-output-string out))))
       (read in)
       (close-port in)
       (close-port out)))))
