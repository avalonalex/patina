;; A loaded file that imports before it uses what it imported (#482).
(import (scheme char))
(define loaded-up (char-upcase #\a))
