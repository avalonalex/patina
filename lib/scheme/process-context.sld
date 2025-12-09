;; (scheme process-context) - R7RS Process Context Library
;;
;; Access to the program's calling context.

(define-library (scheme process-context)
  (import (only (patina internal system)
                command-line
                exit
                emergency-exit
                get-environment-variable
                get-environment-variables))

  (export
    command-line
    exit
    emergency-exit
    get-environment-variable
    get-environment-variables))
