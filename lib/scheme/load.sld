;; (scheme load) - R7RS Load Library
;;
;; Provides the `load` procedure for reading and evaluating files.

(define-library (scheme load)
  (import (patina internal eval))

  (export load))
