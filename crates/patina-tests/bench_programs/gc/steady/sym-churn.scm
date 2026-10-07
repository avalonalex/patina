;; Churn: a fresh symbol per cycle, discarded.
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(include "driver.scm")
(steady-run
 (lambda (i)
   (string->symbol (string-append "sym-churn-" (number->string i)))))
