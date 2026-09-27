(import (scheme base) (scheme read) (prefix (slib pretty-print) p:) (patina compat smoke))
(define (filter-newlines s)
  (let loop ((xs (string->list s)))
    (cond ((null? xs) '()) ((char=? (car xs) #\newline) (cons #t (loop (cdr xs))))
          (else (loop (cdr xs))))))
(check-equal "simple lists use a trailing newline" "(a b c)\n" (p:pretty-print->string '(a b c)))
(check-equal "empty lists and atoms are printable" '("()\n" "42\n")
  (list (p:pretty-print->string '()) (p:pretty-print->string 42)))
(check-equal "dotted lists preserve their tail" "(a . b)\n" (p:pretty-print->string '(a . b)))
(check-equal "quoted forms round trip" '(quote (a b))
  (read (open-input-string (p:pretty-print->string '(quote (a b))))))
(check-equal "strings preserve escapes through readback" "a\n\"b\\c"
  (read (open-input-string (p:pretty-print->string "a\n\"b\\c"))))
(check-equal "vectors retain nested data" '#(1 (a b) "s")
  (read (open-input-string (p:pretty-print->string '#(1 (a b) "s")))))
(check-equal "narrow layout still reads as the original form" '(define (f x) (if x (list 1 2 3) #f))
  (read (open-input-string (p:pretty-print->string '(define (f x) (if x (list 1 2 3) #f)) 12))))
(check-equal "narrow widths add line breaks" #t
  (> (length (filter-newlines (p:pretty-print->string '(alpha beta gamma delta) 10))) 1))
(check-equal "explicit output ports receive the formatted form" "(a b)\n"
  (let ((p (open-output-string))) (p:pretty-print '(a b) p) (get-output-string p)))
(check-equal "pretty printing leaves a supplied port open" #t
  (let ((p (open-output-string))) (p:pretty-print 7 p) (output-port-open? p)))
(smoke-finish)
