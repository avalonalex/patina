(import (scheme base) (scheme read) (prefix (slib pprint-file) p:) (patina compat smoke))
(define (format-source text filter)
  (let ((out (open-output-string)))
    (p:pprint-filter-file (open-input-string text) filter out)
    (get-output-string out)))
(check-equal "multiple datums are printed one per line" "(a b)\n(c d)\n"
  (format-source "(a    b)\n(c d)" (lambda (x) x)))
(check-equal "filter callbacks transform each datum in order" "(wrapped a)\n(wrapped b)\n"
  (format-source "a\nb" (lambda (x) (list 'wrapped x))))
(check-equal "leading and interspersed comments are preserved" "; heading\n(a b)\n; middle\n42\n"
  (format-source "; heading\n(a b)\n; middle\n42" (lambda (x) x)))
(check-equal "comment without a final newline stays intact" ";tail" (format-source ";tail" (lambda (x) x)))
(check-equal "empty input invokes no filter" '("" 0)
  (let ((calls 0))
    (let ((s (format-source "" (lambda (x) (set! calls (+ calls 1)) x)))) (list s calls))))
(check-equal "caller-owned ports remain open" '(#t #t #t)
  (let ((in (open-input-string "1")) (out (open-output-string)))
    (p:pprint-file in out)
    (list (input-port-open? in) (output-port-open? out) (eof-object? (read in)))))
(check-equal "pprint-file uses the identity filter" "(x y)\n"
  (let ((out (open-output-string))) (p:pprint-file (open-input-string "(x   y)") out) (get-output-string out)))
(check-error "filter errors propagate" (format-source "1" (lambda (x) (error "filter failed"))))
(smoke-finish)
