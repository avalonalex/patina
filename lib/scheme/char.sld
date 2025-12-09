;; (scheme char) - R7RS Character Library
;;
;; Unicode character classification and case conversion.

(define-library (scheme char)
  (import (patina internal chars)
          (patina internal strings))

  (export
    ;; Character predicates
    char-alphabetic?
    char-numeric?
    char-whitespace?
    char-upper-case?
    char-lower-case?

    ;; Character case conversion
    char-upcase
    char-downcase
    char-foldcase

    ;; Digit value
    digit-value

    ;; Case-insensitive character comparison
    char-ci=?
    char-ci<?
    char-ci>?
    char-ci<=?
    char-ci>=?

    ;; Case-insensitive string comparison
    string-ci=?
    string-ci<?
    string-ci>?
    string-ci<=?
    string-ci>=?

    ;; String case conversion
    string-upcase
    string-downcase
    string-foldcase))
