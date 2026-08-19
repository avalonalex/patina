;; SRFI 98: An interface to access environment variables
;;
;; R7RS puts both procedures in (scheme process-context); the SRFI name is
;; what pre-R7RS packages import (jkode-sassy in the vendored corpus), so it
;; is provided as a re-export.

(define-library (srfi 98)
  (import (only (scheme process-context)
                get-environment-variable
                get-environment-variables))
  (export get-environment-variable get-environment-variables))
