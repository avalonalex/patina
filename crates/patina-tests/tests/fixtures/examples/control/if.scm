;; Conditional (if) tests - R7RS compliant
(import (scheme base)
        (scheme write))

;; Basic if tests
(write (if #t 1 2)) (newline)              ; 1
(write (if #f 1 2)) (newline)              ; 2
(write (if 0 1 2)) (newline)               ; 1 (0 is truthy)
(write (if '() 1 2)) (newline)             ; 1 (empty list is truthy)
(write (if "false" 1 2)) (newline)         ; 1 (strings are truthy)

;; if without else clause
(write (if #t 42)) (newline)               ; 42
;; (if #f 42) would return unspecified

;; Nested if
(write (if (< 3 5)
           (if (< 2 4) "yes" "no")
           "nope")) (newline)              ; "yes"
