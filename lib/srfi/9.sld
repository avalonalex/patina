;; SRFI 9: defining record types.
;;
;; A re-export, not an implementation. `define-record-type` became R7RS-small
;; 5.5, and the R7RS form is a superset of SRFI 9's: the SRFI requires each
;; field to have an accessor, and R7RS additionally allows a modifier. A
;; program written to the SRFI is therefore accepted unchanged.
;;
;; Bundled for the reason recorded in lib/srfi/PROVENANCE.md.

(define-library (srfi 9)
  (import (only (scheme base) define-record-type))
  (export define-record-type))
