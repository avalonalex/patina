(import (scheme base) (prefix (srfi 28) s:) (patina compat smoke))
(check-equal "empty and literal format strings" '("" "plain text")
  (list (s:format "") (s:format "plain text")))
(check-equal "display and write directives distinguish string quoting"
  "hello / \"hello\"" (s:format "~a / ~s" "hello" "hello"))
(check-equal "write formats compound data" "(1 #t (a . b))"
  (s:format "~s" '(1 #t (a . b))))
(check-equal "literal tilde and newline consume no objects" "~7\n8~"
  (s:format "~~~a~%~s~~" 7 8))
(check-equal "adjacent directives consume arguments in order" "abc42"
  (s:format "~a~a~a~s" 'a "b" #\c 42))
(check-equal "Unicode literals and values are preserved" "λ: café"
  (s:format "λ: ~a" "café"))
(check-error "unfinished escape is rejected" (s:format "trailing ~"))
(check-error "unknown directive is rejected" (s:format "~q" 1))
(check-error "a value directive requires an argument" (s:format "~a ~s" 1))
(smoke-finish)
