;; (scheme write) - R7RS Write Library
;;
;; Output procedures for writing Scheme values.

(define-library (scheme write)
  (import (only (patina internal io)
                write
                write-shared
                write-simple
                display))

  (export
    write
    write-shared
    write-simple
    display))
