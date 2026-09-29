;; #359: R7RS 2.1 requires identifier and character-name folding as if by
;; string-foldcase. Folding can expand a character, and differs from lowercase.
;; Each read below has its own directive; persistence between calls is #360.
;; Measured 2026-09-29: Chibi 0.12 and Gauche 0.9.15 fold ASCII only in
;; identifiers and accept mixed-case character names even without folding.
;; Each passes 8/15; DIVERGENCES.tsv records the seven differing rows.
(import (scheme base) (scheme char) (scheme read) (srfi 64))

(test-begin "fold-case")

(define (read-text text) (read (open-input-string text)))
(define (read-folded text) (read-text (string-append "#!fold-case " text)))
(define (folded-name text) (symbol->string (read-folded text)))

(test-equal "ASCII identifiers fold" "abc" (folded-name "ABC"))
(test-equal "sharp s uses full folding"
  '("strasse" "strasse" "ss")
  (map folded-name '("Straße" "STRASSE" "ẞ")))
(test-equal "sigma folding is independent of word position"
  '("οδοσ" "οσοσ" "σσσ")
  (map folded-name '("ΟΔΟΣ" "ΟΣΟΣ" "Σσς")))
(test-equal "full folding can expand one character into several"
  '("ffi" "i\x0307;") (map folded-name '("ﬃ" "İ")))
(test-equal "identifier folding agrees with string-foldcase"
  (map string->symbol (map string-foldcase '("Straße" "ΟΔΟΣ" "Σσς" "ﬃ" "İ")))
  (map read-folded '("Straße" "ΟΔΟΣ" "Σσς" "ﬃ" "İ")))
(test-equal "numeric candidates that become identifiers fold fully"
  '("+istrasse" "-iσ") (map folded-name '("+IStraße" "-IΣ")))

(test-equal "all named characters accept folded case"
  '(#\alarm #\backspace #\delete #\escape #\newline #\null #\return #\space #\tab)
  (map read-folded '("#\\ALARM" "#\\BackSpace" "#\\DELETE" "#\\Escape"
                     "#\\NEWLINE" "#\\Null" "#\\RETURN" "#\\Space" "#\\TAB")))
(test-equal "hex character spellings fold without changing their values"
  '(#\A #\Å #\Σ)
  (map read-folded '("#\\X41" "#\\XC5" "#\\X3A3")))
(test-equal "single character literals keep their original characters"
  '(#\A #\Σ #\ß #\ẞ #\İ #\( #\|)
  (map read-folded '("#\\A" "#\\Σ" "#\\ß" "#\\ẞ" "#\\İ" "#\\(" "#\\|")))
(test-equal "strings and barred identifiers preserve their spelling"
  '("Straße ΟΔΟΣ" "Straße" "Σ" "Straße")
  (let ((values (read-folded "(\"Straße ΟΔΟΣ\" |Straße| |Σ| |Stra\\xdf;e|)")))
    (cons (car values) (map symbol->string (cdr values)))))
(test-equal "default and no-fold-case preserve identifier spelling"
  '("Straße" "ΟΔΟΣ" "Straße" "ΟΔΟΣ")
  (map (lambda (text) (symbol->string (read-text text)))
       '("Straße" "ΟΔΟΣ" "#!no-fold-case Straße" "#!no-fold-case ΟΔΟΣ")))
(test-equal "no-fold-case restores case-sensitive character names"
  '(#t #t #t #t)
  (map (lambda (text)
         (guard (e (else (read-error? e))) (read-text text) #f))
       '("#\\NEWLINE" "#\\Space"
         "#!fold-case #!no-fold-case #\\NEWLINE"
         "#!fold-case #!no-fold-case #\\Space")))
(test-equal "folding still rejects invalid character spellings"
  '(#t #t #t)
  (map (lambda (text)
         (guard (e (else (read-error? e))) (read-folded text) #f))
       '("#\\BOGUS" "#\\Newlines" "#\\XGG")))
(test-equal "directives select folding for each token within a datum"
  '("strasse" "Straße" "strasse" "+istrasse" "+IStraße")
  (map symbol->string
       (read-text "(#!fold-case Straße #!no-fold-case Straße #!fold-case Straße +IStraße #!no-fold-case +IStraße)")))
(test-equal "character-name folding follows directive changes"
  '(#\space #\space #\newline)
  (read-text "(#!fold-case #\\Space #!no-fold-case #\\space #!fold-case #\\NEWLINE)"))

(test-end)
