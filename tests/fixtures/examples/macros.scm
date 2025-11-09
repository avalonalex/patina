;; Macro System Examples
;; =====================
;; This file demonstrates various capabilities of the Patina macro system.
;;
;; NOTE: These examples are designed to work with the current set of
;; primitives. Use `cargo run --release` and paste these examples into the REPL.

;; Example 1: Simple Conditional - 'when' macro
;; ---------------------------------------------
;; The 'when' macro provides a more readable alternative to if when there's no else clause.

(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

;; Test: true condition returns value
(when #t 42)
;; => 42

;; Test: false condition returns #<unspecified>
(when #f 99)
;; => #<unspecified>

;; Test: multiple body forms - returns last value
(when #t 1 2 3)
;; => 3

;; Example 2: Opposite Conditional - 'unless' macro
;; -------------------------------------------------
;; The 'unless' macro inverts the test - runs body when test is false.

(define-syntax unless
  (syntax-rules ()
    ((unless test body ...)
     (if (not test) (begin body ...)))))

;; Test: false condition runs body
(unless #f 42)
;; => 42

;; Test: true condition returns #<unspecified>
(unless #t 99)
;; => #<unspecified>

;; Example 3: Variable Swap - Demonstrates Hygiene
;; -----------------------------------------------
;; This macro introduces a temporary variable named 'temp'.
;; Due to hygiene, it won't capture any user variable named 'temp'.

(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((temp a))
       (set! a b)
       (set! b temp)))))

;; Setup
(define x 1)
(define y 2)
(define temp 999)

;; Swap x and y
(swap! x y)

;; Check results
x  ;; => 2
y  ;; => 1
temp  ;; => 999 (unchanged! hygiene prevented capture)

;; Example 4: Simple List Construction
;; -----------------------------------
;; A macro that creates a list with duplicated first element

(define-syntax dup-first
  (syntax-rules ()
    ((dup-first x)
     (cons x (cons x '())))))

(dup-first 5)
;; => (5 5)

;; Example 5: Conditional Expression Builder
;; -----------------------------------------
;; A macro that builds if expressions with specific structure

(define-syntax test-and-double
  (syntax-rules ()
    ((test-and-double val)
     (if (> val 0)
         (* val 2)
         0))))

(test-and-double 5)
;; => 10

(test-and-double -3)
;; => 0

;; Example 6: Pattern Matching with Ellipsis
;; -----------------------------------------
;; A macro that demonstrates ellipsis pattern matching

(define-syntax make-list
  (syntax-rules ()
    ((make-list elem ...)
     (cons elem (cons ... '())))))

;; Note: This requires proper ellipsis expansion support
