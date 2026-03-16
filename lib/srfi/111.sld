;; SRFI 111: Boxes
;;
;; Reference implementation from https://srfi.schemers.org/srfi-111/
;; Original author: John Cowan
;; License: MIT (see README.md in this directory)

(define-library (srfi 111)
  (import (scheme base))
  (export box box? unbox set-box!)

  (begin
    (define-record-type box-type
      (box value)
      box?
      (value unbox set-box!))))
