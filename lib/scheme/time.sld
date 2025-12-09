;; (scheme time) - R7RS Time Library
;;
;; Time-related procedures.

(define-library (scheme time)
  (import (patina internal time))

  (export
    current-second
    current-jiffy
    jiffies-per-second))
