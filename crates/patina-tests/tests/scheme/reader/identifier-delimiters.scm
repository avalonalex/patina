;; #421: stop unescaped tokens at quote syntax and vertical bars, matching
;; Gauche 0.9.15. Chibi 0.12 agrees on identifiers followed by apostrophes or
;; commas, but not backticks or bars, and has narrower number/character/dot
;; boundaries. Those measured differences live in DIVERGENCES.tsv.
;; Names containing punctuation remain available inside |...|. Check datum
;; structure rather than writer spelling.
(import (scheme base) (scheme read) (scheme write) (srfi 64))

(define (read-all text)
  (let ((p (open-input-string text)))
    (let loop ((data '()))
      (let ((datum (read p)))
        (if (eof-object? datum) (reverse data) (loop (cons datum data)))))))

(test-begin "identifier-delimiters")

;; These literal forms exercise the program reader as well as `read` below.
(test-equal "quote ends an identifier in source" '(a (quote b)) '(a'b))
(test-equal "quasiquote ends an identifier in source" '(a (quasiquote b)) '(a`b))
(test-equal "unquote ends an identifier in source" '(a (unquote b)) '(a,b))
(test-equal "unquote-splicing ends an identifier in source"
  '(a (unquote-splicing b)) '(a,@b))
(test-equal "a vertical bar begins the next identifier" '(a b c) '(a|b|c))

(test-equal "successive reads stop before quote" '(a (quote b)) (read-all "a'b"))
(test-equal "successive reads stop before quasiquote" '(a (quasiquote b)) (read-all "a`b"))
(test-equal "successive reads stop before unquote" '(a (unquote b)) (read-all "a,b"))
(test-equal "successive reads preserve unquote-splicing"
  '(a (unquote-splicing b)) (read-all "a,@b"))
(test-equal "successive reads stop before bars" '(a b c) (read-all "a|b|c"))
(test-equal "Unicode identifiers use the same boundaries"
  '(λ (quote β) γ) (read-all "λ'β|γ|"))

(test-equal "decimal numbers stop at quote syntax"
  '(12 (quote a) -3 (quasiquote b) 1/2 (unquote c) 4 (unquote-splicing d))
  (read-all "12'a -3`b 1/2,c 4,@d"))
(test-equal "prefixed numbers stop at quote syntax"
  '(16 (quote a) 2 (quasiquote b) 3 (unquote c) 4 (unquote-splicing d))
  (read-all "#x10'a #b10`b #e3,c #o4,@d"))
(test-equal "numbers stop before a barred identifier"
  '(12 a 16 b) (read-all "12|a|#x10|b|"))
(test-equal "a dot before quote is a dotted-list marker"
  '(a quote b) (read (open-input-string "(a .'b)")))
(test-equal "a dot before bars is a dotted-list marker"
  '(a . b) (read (open-input-string "(a .|b|)")))

(test-equal "character names stop at quote syntax"
  (list #\space '(quote a) #\newline '(quasiquote b) #\x41 '(unquote c))
  (read-all "#\\space'a #\\newline`b #\\x41,c"))
(test-equal "delimiter character literals still read as characters"
  (list #\' 'a #\` 'b #\, 'c #\| 'd)
  (read-all "#\\' a #\\` b #\\, c #\\| d"))
(test-equal "quote character literals still need a following delimiter"
  '(#t #t #t)
  (map (lambda (text) (guard (e (else (read-error? e))) (read-all text) #f))
       '("#\\'a" "#\\`a" "#\\,a")))

(test-equal "quote syntax terminates a case-folding directive"
  '((quote a)) (read-all "#!fold-case'A"))
(test-equal "a barred identifier terminates a case-folding directive"
  '(A) (read-all "#!fold-case|A|"))

(test-equal "bars preserve punctuation inside a name"
  '("a'b" "a`b" "a,b" "a,@b" "a|b" "a[b]")
  (map symbol->string '(|a'b| |a`b| |a,b| |a,@b| |a\|b| |a[b]|)))
(test-equal "write and read preserve names containing delimiters"
  '("a'b" "a`b" "a,b" "a,@b" "a|b")
  (map (lambda (name)
         (let ((p (open-output-string)))
           (write (string->symbol name) p)
           (symbol->string (read (open-input-string (get-output-string p))))))
       '("a'b" "a`b" "a,b" "a,@b" "a|b")))

(test-end)
