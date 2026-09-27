(import (scheme base) (srfi 37))
(import (patina compat smoke))

;; Two independent seeds make callback arity and returned values observable.
(define (record-option option name argument events count)
  (values (cons (list 'option name argument) events) (+ count 1)))
(define verbose (option '(#\v "verbose") #f #f record-option))
(define output (option '(#\o "output") #t #f record-option))
(define color (option '(#\c "color") #f #t record-option))
(define (parse arguments)
  (call-with-values
    (lambda ()
      (args-fold arguments (list verbose output color)
        (lambda (option name argument events count)
          (values (cons (list 'unknown name argument
                              (option-required-arg? option)) events)
                  (+ count 1)))
        (lambda (operand events count)
          (values (cons (list 'operand operand) events) (+ count 1)))
        '() 0))
    (lambda (events count) (list (reverse events) count))))

(check-equal "option accessors" '((#\o "output") #t #f #t)
  (list (option-names output) (option-required-arg? output)
        (option-optional-arg? output) (eq? record-option (option-processor output))))
(check-equal "empty arguments preserve both seeds" '(() 0) (parse '()))
(check-equal "clustered short options and attached required argument"
  '(((option #\v #f) (option #\v #f) (option #\o "file")) 3)
  (parse '("-vvofile")))
(check-equal "required arguments may be attached or separate"
  '(((option "output" "a") (option "output" "b") (option #\o "c")) 3)
  (parse '("--output=a" "--output" "b" "-o" "c")))
(check-equal "optional arguments do not consume the next operand"
  '(((option "color" "always") (option #\c "auto")
     (option "color" #f) (operand "file")) 4)
  (parse '("--color=always" "-cauto" "--color" "file")))
(check-equal "operands interleave with options until the terminator"
  '(((operand "first") (option #\v #f) (operand "-")
     (operand "--verbose") (operand "last")) 5)
  (parse '("first" "-v" "-" "--" "--verbose" "last")))
(check-equal "unknown options reach their callback"
  '(((unknown #\x #f #f) (unknown "mystery" #f #f)
     (unknown "other" "value" #t)) 3)
  (parse '("-x" "--mystery" "--other=value")))
(check-equal "missing required argument is passed to the callback as false"
  '(((option "output" #f)) 1)
  (parse '("--output")))
(smoke-finish)
