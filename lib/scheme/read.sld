;; (scheme read) - R7RS Read Library
;;
;; Input procedures for reading Scheme values.

(define-library (scheme read)
  (import (only (patina internal io)
                read))

  (export
    read))
