;; (scheme repl) - R7RS REPL Library
;;
;; Provides `interaction-environment` for access to the REPL environment.

(define-library (scheme repl)
  (import (patina internal eval))

  (export interaction-environment))
