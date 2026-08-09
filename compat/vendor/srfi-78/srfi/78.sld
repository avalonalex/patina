;Time-stamp: <2019-10-26 22:10:26 lockywolf>
#;(Written-by Lockywolf gmail.com)
#;(Year 2019)
#;(This file blindly loads the reference implementation.)
(define-library (srfi 78)
  (import (srfi 23)  (srfi 42)
	  (scheme r5rs))
  (export check check-ec check-report
	  check-set-mode!)
  (include "78/check.scm"))
