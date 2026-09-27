(import (scheme base) (scheme read) (scheme write)
        (slib string-port) (patina compat smoke))
(check-equal "collect output from a callback" "hello (1 2)"
  (call-with-output-string
    (lambda (port) (display "hello " port) (write '(1 2) port))))
(check-equal "empty output ignores callback return value" ""
  (call-with-output-string (lambda (port) 'unused)))
(check-equal "input callback returns its result" '(alpha 42 #t)
  (call-with-input-string "alpha 42"
    (lambda (port)
      (let* ((first (read port)) (second (read port)))
        (list first second (eof-object? (read port)))))))
(check-equal "input callback returns multiple values" '(left right)
  (call-with-values
    (lambda () (call-with-input-string "" (lambda (port) (values 'left 'right))))
    list))
(define saved-port #f)
(call-with-input-string "data" (lambda (port) (set! saved-port port)))
(check-equal "input port closes after the callback" #f (input-port-open? saved-port))
(check-error "callback errors propagate"
  (call-with-output-string (lambda (port) (error "smoke callback failure"))))
(smoke-finish)
