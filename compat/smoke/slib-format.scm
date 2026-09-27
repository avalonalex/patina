(import (scheme base) (scheme write) (slib format))
(import (patina compat smoke))
(check-equal "display and write directives" "hello \"world\""
  (format #f "~a ~s" "hello" "world"))
(check-equal "integer radix directives" "42 2a 101010"
  (format #f "~d ~x ~b" 42 42 42))
(check-equal "iteration" "1, 2, 3" (format #f "~{~a~^, ~}" '(1 2 3)))
(define out (open-output-string))
(format out "~a~%" "line")
(check-equal "explicit output port" "line\n" (get-output-string out))
(smoke-finish)
