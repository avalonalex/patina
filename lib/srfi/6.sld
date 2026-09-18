;; SRFI 6: basic string ports.
;;
;; A re-export, not an implementation. All three procedures became R7RS-small
;; 6.13 verbatim, so `(scheme base)` already has them and a second definition
;; would be two to keep in step.
;;
;; Bundled because a program written against the pre-R7RS name should load
;; rather than fail over a library whose functionality is present under
;; another name. See lib/srfi/PROVENANCE.md.

(define-library (srfi 6)
  (import (only (scheme base)
                open-input-string open-output-string get-output-string))
  (export open-input-string open-output-string get-output-string))
