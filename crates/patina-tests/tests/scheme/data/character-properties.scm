;; Character properties — R7RS 6.6, Larceny family 7.
;; char-numeric? means Numeric_Type=Decimal, not every character with a
;; numeric value. Its true results must have a digit-value from 0 through 9.

(import (scheme base) (scheme char) (srfi 64))

(test-begin "character-properties")

(define (numeric-properties chars)
  (map (lambda (c) (list (char-numeric? c) (digit-value c))) chars))

(test-equal "superscripts and subscripts are not decimal digits"
  '((#f #f) (#f #f) (#f #f))
  (numeric-properties '(#\xB2 #\xB3 #\x2082)))

(test-equal "fractions are not decimal digits"
  '((#f #f) (#f #f))
  (numeric-properties '(#\xBD #\x2153)))

(test-equal "Roman numerals are not decimal digits"
  '((#f #f) (#f #f))
  (numeric-properties '(#\x2163 #\x2173)))

(test-equal "circled numbers are not decimal digits"
  '((#f #f) (#f #f))
  (numeric-properties '(#\x2461 #\x24EA)))

(test-equal "decimal digits retain their values across scripts"
  '((#t 0) (#t 9) (#t 2) (#t 3) (#t 4) (#t 5))
  (numeric-properties '(#\0 #\9 #\x662 #\x6F3 #\x96A #\xFF15)))

(test-equal "supplementary decimal digits retain their values"
  '((#t 0) (#t 9) (#t 2) (#t 9))
  (numeric-properties '(#\x104A0 #\x104A9 #\x1D7D0 #\x1D7FF)))

;; Kawi was added in Unicode 15.0. Patina's decimal-digit table claimed that
;; version but omitted this block. Oracle version differences are registered.
(test-equal "Kawi decimal digits have values zero through nine"
  '((#t 0) (#t 1) (#t 2) (#t 3) (#t 4)
    (#t 5) (#t 6) (#t 7) (#t 8) (#t 9))
  (numeric-properties
    (map integer->char '(#x11F50 #x11F51 #x11F52 #x11F53 #x11F54
                        #x11F55 #x11F56 #x11F57 #x11F58 #x11F59))))

(test-equal "non-digits beside decimal blocks stay non-numeric"
  '((#f #f) (#f #f) (#f #f) (#f #f) (#f #f) (#f #f))
  (numeric-properties '(#\x2F #\x3A #\x65F #\x66A #\x11F4F #\x11F5A)))

(test-end)
