;; The primitive expression types — R7RS §4.1: variable references, `quote`,
;; `lambda`, `if`, `define`, `set!` and `begin`.
;;
;; Migrated from `crates/patina-tests/tests/compliance/primitives.rs` (#193),
;; which is deleted. Its rows were adapted from chibi's `r7rs-tests.scm`, most
;; of them the report's own examples.
;;
;; The Rust file compared printed forms. Here `test-equal` compares values with
;; `equal?`, which says the same thing for these rows with one exception it
;; names: a quoted `quote` form, which the Rust file printed as `'a`. Whether a
;; writer abbreviates `(quote a)` is printing latitude — the register records
;; chibi declining to — and the row is about what the reader and `quote`
;; produce, so it compares the list.

(import (scheme base) (srfi 64))

(test-begin "core-forms")

;; ── 4.1.1 Variable references ──────────────────────────────────────────────

(define x 28)
(test-equal "a variable reference" 28 x)

;; ── 4.1.2 Literal expressions ──────────────────────────────────────────────

(test-equal "quote of a symbol" 'a (quote a))
(test-equal "the quote abbreviation" 'a 'a)
(test-equal "a quoted list is not evaluated" (list '+ 1 2) '(+ 1 2))
(test-equal "the long form of a quoted list" (list '+ 1 2) (quote (+ 1 2)))
(test-equal "a quoted quote form is a list" (list 'quote 'a) '(quote a))
(test-equal "a doubly quoted symbol is the same list" (list 'quote 'a) ''a)
(test-equal "a string is self-evaluating" "abc" "abc")
(test-equal "a number is self-evaluating" 145932 145932)
(test-equal "#t is self-evaluating" #t #t)
(test-equal "#f is self-evaluating" #f #f)

;; ── 4.1.4 Procedures ───────────────────────────────────────────────────────

(test-equal "a lambda applied directly" 8 ((lambda (x) (+ x x)) 4))

(define reverse-subtract
  (lambda (x y) (- y x)))
(test-equal "a two-argument lambda" 3 (reverse-subtract 7 10))

(define add4
  (let ((x 4))
    (lambda (y) (+ x y))))
(test-equal "a lambda closes over its environment" 10 (add4 6))

(test-equal "a variadic lambda takes every argument as a list" '(3 4 5 6)
  ((lambda x x) 3 4 5 6))
(test-equal "a dotted formal list takes the rest" '(5 6)
  ((lambda (x y . z) z) 3 4 5 6))

;; ── 4.1.5 Conditionals ─────────────────────────────────────────────────────

(test-equal "if with a true test" 'yes (if (> 3 2) 'yes 'no))
(test-equal "if with #t" 'yes (if #t 'yes 'no))
(test-equal "if with a false test" 'no (if (> 2 3) 'yes 'no))
(test-equal "if with #f" 'no (if #f 'yes 'no))
(test-equal "if evaluates only the consequent" 1 (if (> 3 2) (- 3 2) (+ 3 2)))
(test-equal "if can select a procedure" 12 ((if #f + *) 3 4))

;; ── 4.1.6 Assignments, and 5.3 definitions ─────────────────────────────────

(define ten 10)
(test-equal "define binds a variable" 10 ten)

(define two 2)
(define three 3)
(test-equal "several definitions" 5 (+ two three))

(define changed 10)
(set! changed 20)
(test-equal "set! changes a binding" 20 changed)

;; R7RS §4.1.6 makes assigning an unbound variable an error; Patina, chibi and
;; Gauche all signal it at run time.
(test-error "set! of an unbound variable is an error" #t
  (set! undefined-var 42))

;; ── 4.2.3 Sequencing ───────────────────────────────────────────────────────

;; `begin` at top level splices, so its definitions are visible after it.
(begin (define a 5) (define b 10))
(test-equal "a top-level begin's definitions" 15 (+ a b))
(test-equal "begin returns its last value" 3 (begin 1 2 3))

(test-end)
