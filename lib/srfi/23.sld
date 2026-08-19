;; SRFI 23: Error reporting mechanism
;;
;; R7RS base subsumes it — `error` there takes the same message + irritants
;; shape — but pre-R7RS packages import the SRFI by name (in the vendored
;; corpus, SRFI 78's reference implementation reaches for it), so the name is
;; provided as a re-export.

(define-library (srfi 23)
  (import (only (scheme base) error))
  (export error))
