(import (scheme base) (scheme read) (slib generic-write) (patina compat smoke))
(define (render object display? width)
  (let* ((port (open-output-string))
         (column (generic-write object display? width
                   (lambda (text) (write-string text port) #t))))
    (list (get-output-string port) column)))
(check-equal "write mode escapes strings and counts columns" '("\"a\\\"b\"" 6)
  (render "a\"b" #f #f))
(check-equal "display mode writes strings and characters directly" '("(a  )" 5)
  (render '("a" #\space) #t #f))
(check-equal "quote abbreviations" '("'(1 2)" 6) (render '(quote (1 2)) #f #f))
(check-equal "dotted lists and vectors" '("(a . #(1 #t))" 13)
  (render '(a . #(1 #t)) #f #f))
(define datum '(define (square x) (* x x)))
(define pretty (car (render datum #f 12)))
(check-equal "narrow pretty printing preserves the datum" datum
  (read (open-input-string pretty)))
(define calls 0)
(define stopped
  (generic-write '(1 2 3) #f #f
    (lambda (text) (set! calls (+ calls 1)) #f)))
(check-equal "callback can stop writing" '(#f 1) (list stopped calls))
(check-equal "reverse append handles empty pieces" "abcd"
  (reverse-string-append '("cd" "" "ab")))
(smoke-finish)
