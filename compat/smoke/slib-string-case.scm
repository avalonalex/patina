(import (scheme base) (slib string-case) (patina compat smoke))
(define original (string-copy "hELLO, sCHEME wORLD!"))
(check-equal "capitalize words" "Hello, Scheme World!" (string-capitalize original))
(check-equal "nonmutating capitalization preserves its input"
  "hELLO, sCHEME wORLD!" original)
(string-capitalize! original)
(check-equal "in-place capitalization" "Hello, Scheme World!" original)
(check-equal "case-insensitive symbol conversion" 'mixed (string-ci->symbol "MiXeD"))
(check-equal "symbol concatenation accepts mixed components" 'item-42
  (symbol-append 'item "-" 42 #f))
(check-equal "expand acronym and word boundaries" "parse_HTTP_Response"
  (StudlyCapsExpand "parseHTTPResponse" #\_))
(check-error "symbol concatenation rejects unsupported components"
  (symbol-append '(item)))
(smoke-finish)
