;; (scheme file) - R7RS File Library
;;
;; File I/O operations.

(define-library (scheme file)
  (import (scheme base)
          (only (patina internal io)
                open-input-file
                open-output-file
                open-binary-input-file
                open-binary-output-file
                call-with-input-file
                call-with-output-file
                file-exists?
                delete-file))

  (export
    open-input-file
    open-output-file
    open-binary-input-file
    open-binary-output-file
    call-with-input-file
    call-with-output-file
    with-input-from-file
    with-output-to-file
    file-exists?
    delete-file)

  (include "file/redirect.scm"))
