;; Basic when/unless macros
;; Tests simple pattern matching and ellipsis

(import (scheme base) (scheme write))

;; when macro - execute body if test is true
(define-syntax when
  (syntax-rules ()
    ((when test result1 result2 ...)
     (if test (begin result1 result2 ...)))))

;; Test 1: when with single expression
(display "Test 1: when with single expression\n")
(when #t (display "  Success!\n"))  ; Should print
(when #f (display "  Fail!\n"))     ; Should not print

;; Test 2: when with multiple expressions
(display "\nTest 2: when with multiple expressions\n")
(define x 0)
(when #t
  (set! x 1)
  (set! x (+ x 1))
  (set! x (+ x 1)))
(display "  x = ")
(display x)  ; Should be 3
(display "\n")

;; Test 3: when returns value of last expression
(display "\nTest 3: when return value\n")
(define result (when #t 1 2 3))
(display "  result = ")
(display result)  ; Should be 3
(display "\n")

;; unless macro - execute body if test is false
(define-syntax unless
  (syntax-rules ()
    ((unless test result1 result2 ...)
     (if (not test) (begin result1 result2 ...)))))

;; Test 4: unless with single expression
(display "\nTest 4: unless with single expression\n")
(unless #f (display "  Success!\n"))  ; Should print
(unless #t (display "  Fail!\n"))     ; Should not print

;; Test 5: unless with multiple expressions
(display "\nTest 5: unless with multiple expressions\n")
(set! x 0)
(unless #f
  (set! x 1)
  (set! x (+ x 1))
  (set! x (+ x 1)))
(display "  x = ")
(display x)  ; Should be 3
(display "\n")

;; Test 6: when/unless with exactly one body form
(display "\nTest 6: Single body form\n")
(define single-result (when #t 42))
(display "  single-result = ")
(display single-result)  ; Should be 42
(display "\n")

(display "\nAll when/unless tests completed!\n")
