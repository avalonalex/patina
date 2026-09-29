;; #358/#368: classify the whole token, and preserve symbol identity through
;; every writer. R7RS 7.1.1 permits these peculiar identifiers even though
;; their prefixes also begin numbers. The writer may conservatively bar them.
;;
;; Measured 2026-09-29: Patina VM and tree-walker pass 12/12.
;; Chibi 0.12 passes 10/12, rejecting the +inf.0-prefixed
;; identifiers and treating four numeric spellings as symbols. Gauche 0.9.15
;; passes 11/12, accepting three malformed numbers as identifier extensions.
;; DIVERGENCES.tsv records the individual spellings and grammar evidence.

(import (scheme base) (scheme read) (scheme write) (srfi 64))

(test-begin "peculiar-identifiers")

(define (+inc x) (+ x 1))
(define (-index x) (- x 1))
(define +nan.0abc 42)
(test-equal "a procedure beginning with +i" 2 (+inc 1))
(test-equal "a procedure beginning with -i" 1 (-index 2))
(test-equal "a binding beginning with +nan.0" 42 +nan.0abc)

(define names
  '("+inf" "+id" "-in" "+nan.0abc" "+inf.0abc" "-inf.0abc"
    "+iota" "-index" "+INF" "-IN" "+NaN.0abc" "+inf.0i-tail"))

(test-equal "numeric-looking identifiers read as symbols" names
  (map (lambda (name) (symbol->string (read (open-input-string name)))) names))
(test-equal "identifiers are not numeric strings" (map (lambda (name) #f) names)
  (map string->number names))

(define (round-trips writer)
  (map
    (lambda (name)
      (let ((out (open-output-string)))
        (writer (string->symbol name) out)
        (let* ((in (open-input-string (get-output-string out)))
               (value (read in)))
          (and (eq? value (string->symbol name)) (eof-object? (read in))))))
    names))
(test-equal "write preserves symbol identity" (map (lambda (name) #t) names)
  (round-trips write))
(test-equal "write-simple preserves symbol identity" (map (lambda (name) #t) names)
  (round-trips write-simple))
(test-equal "write-shared preserves symbol identity" (map (lambda (name) #t) names)
  (round-trips write-shared))

(define numbers
  '("+i" "-i" "+I" "-I" "+inf.0" "-inf.0" "+nan.0" "-nan.0"
    "+inf.0i" "-inf.0i" "+NaN.0i" "+inf.0+2i" "+inf.0-i"
    "+nan.0@1" "+1" "-.5"))
(test-equal "valid numeric spellings stay numbers" (map (lambda (text) #t) numbers)
  (map (lambda (text) (number? (read (open-input-string text)))) numbers))

(test-equal "malformed numeric tokens stay read errors" '(#t #t #t #t)
  (map (lambda (text)
         (guard (ex (else (read-error? ex)))
           (read (open-input-string text)) #f))
       '("12abc" "+12abc" "-.5abc" "#d+inf.0abc")))

(test-equal "folding applies to the first ambiguous token after a directive"
  "+inc" (symbol->string (read (open-input-string "#!fold-case +INC"))))
(test-equal "no-fold-case restores the spelling"
  '(+inc +INC)
  (read (open-input-string "(#!fold-case +INC #!no-fold-case +INC)")))

(test-end)
