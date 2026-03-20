;; SRFI 8: receive — Binding to multiple values
;;
;; Reference: https://srfi.schemers.org/srfi-8/
;; Author: John David Stone
;; License: MIT

(define-library (srfi 8)
  (import (scheme base))
  (export receive)
  (begin
    (define-syntax receive
      (syntax-rules ()
        ((receive formals expr body ...)
         (call-with-values (lambda () expr)
           (lambda formals body ...)))))))
