;; `case-lambda` — a procedure that dispatches on how many arguments it got.
;;
;; R7RS §4.2.9, provided by `(scheme case-lambda)`, which Patina implements as a
;; macro in Scheme rather than a special form. Migrated from
;; `crates/patina-tests/tests/case_lambda.rs` (#193 Phase 1).
;;
;; The interesting rows are arity selection: a clause list is tried in order and
;; the first arity that fits wins, so `(() 'zero)` before `(args …)` and
;; `((x y) …)` before `((x y . z) …)` are what distinguish working dispatch from
;; always falling through to the variadic clause. That is why the tables below
;; call one procedure at every arity rather than testing each clause alone.
;;
;; **Definitions are at top level, deliberately.** The Rust rows each ran as
;; their own program with a top-level `define`, and a top-level self-reference
;; resolves through the global environment at call time while an internal
;; `define` is `letrec*` and compiles to a local slot in the register VM.
;; Wrapping these in `(let () (define …) …)` would quietly move the recursive
;; rows off the path they were written to cover, so each procedure keeps a
;; distinct top-level name instead.

(import (scheme base) (scheme case-lambda) (srfi 64))

(test-begin "case-lambda")

;; ── Arity selection ─────────────────────────────────────────────────────────

(define zero-only (case-lambda (() 'zero)))
(test-equal "zero arguments" 'zero (zero-only))

(define one-only (case-lambda ((x) x)))
(test-equal "one argument" 42 (one-only 42))

(define two-only (case-lambda ((x y) (cons x y))))
(test-equal "two arguments" '(1 . 2) (two-only 1 2))

;; The first clause whose arity fits wins, so calling one procedure at every
;; arity is what shows dispatch working rather than one clause matching always.
(define three-clauses (case-lambda (() 'zero) ((x) x) ((x y) (cons x y))))
(test-equal "several clauses, called at each arity" '(zero 1 (1 . 2))
  (list (three-clauses) (three-clauses 1) (three-clauses 1 2)))

;; ── Variadic clauses ────────────────────────────────────────────────────────

(define variadic-only (case-lambda (args args)))
(test-equal "a lone variadic clause collects its arguments" '(1 2 3)
  (variadic-only 1 2 3))
(test-equal "…and accepts none" '() (variadic-only))

(define fixed-plus-rest (case-lambda ((x . rest) (cons x rest))))
(test-equal "fixed head plus a rest tail" '(1 2 3) (fixed-plus-rest 1 2 3))

(define fixed-then-variadic
  (case-lambda (() 'zero)
               ((x) (list 'one x))
               ((x y) (list 'two x y))
               ((x y . z) (list 'more x y z))))
(test-equal "fixed clauses before a variadic one"
  '(zero (one 1) (two 1 2) (more 1 2 (3)) (more 1 2 (3 4)))
  (list (fixed-then-variadic) (fixed-then-variadic 1) (fixed-then-variadic 1 2)
        (fixed-then-variadic 1 2 3) (fixed-then-variadic 1 2 3 4)))

;; ── R7RS's own examples, as chibi's suite states them ───────────────────────

(define any-arity
  (case-lambda (() 'zero)
               ((x) x)
               ((x y) (cons x y))
               ((x y z) (list x y z))
               (args (cons 'many args))))
(test-equal "any-arity" '(zero 1 (1 . 2) (1 2 3) (many 1 2 3 4))
  (list (any-arity) (any-arity 1) (any-arity 1 2)
        (any-arity 1 2 3) (any-arity 1 2 3 4)))

(define rest-arity
  (case-lambda (() '(zero))
               ((x) (list 'one x))
               ((x y) (list 'two x y))
               ((x y . z) (list 'more x y z))))
(test-equal "rest-arity" '((zero) (one 1) (two 1 2) (more 1 2 (3)) (more 1 2 (3 4)))
  (list (rest-arity) (rest-arity 1) (rest-arity 1 2)
        (rest-arity 1 2 3) (rest-arity 1 2 3 4)))

;; ── Closure behaviour ───────────────────────────────────────────────────────

;; Every clause closes over the same environment, not one apiece.
(define captured 10)
(define reads-captured (case-lambda (() captured) ((y) (+ captured y))))
(test-equal "clauses share the enclosing environment" '(10 15)
  (list (reads-captured) (reads-captured 5)))

;; And over the same mutable state: the counter's two clauses read and write one
;; `n`, so the sequence is 0, 5, 8, 8 rather than each clause getting its own
;; binding.
;;
;; **`let*`, not `(list (c) (c 5) …)`.** R7RS leaves argument evaluation order
;; unspecified, and chibi evaluates right to left — the argument-position
;; version answers `(8 8 3 0)` there and `(0 5 8 8)` here, so it would report a
;; difference between implementations that is not one. Sequencing the calls is
;; what makes this row mean the same thing everywhere.
(define (make-counter n)
  (case-lambda (() n) ((x) (set! n (+ n x)) n)))
(test-equal "clauses share mutable state" '(0 5 8 8)
  (let* ((c (make-counter 0))
         (a (c))
         (b (c 5))
         (d (c 3))
         (e (c)))
    (list a b d e)))

;; ── As an ordinary value ────────────────────────────────────────────────────

(define (apply-twice f) (list (f 1) (f 1 2)))
(test-equal "passed as an argument" '(1 3)
  (apply-twice (case-lambda ((x) (* x x)) ((x y) (+ x y)))))

(define (make-adder n)
  (case-lambda ((x) (+ n x)) ((x y) (+ n x y))))
(test-equal "returned from a procedure" '(8 11)
  (let* ((add5 (make-adder 5))
         (a (add5 3))
         (b (add5 2 4)))
    (list a b)))

;; ── Errors ──────────────────────────────────────────────────────────────────

;; Only the *catchable* half is here. `case_lambda.rs` also asserted that the
;; same call fails an unguarded top-level program, which `test-error` cannot
;; express — it runs its body inside `call/cc` and `with-exception-handler`
;; (SRFI 64's `%test-error`), so it checks the same routing this row does. That
;; half stayed in Rust, per docs/TEST_ORGANIZATION.md's rule that "an error
;; escapes an unguarded program" is observable only from outside it.
(define two-clauses (case-lambda ((x) x) ((x y) (cons x y))))
(test-error "no clause matches the call" #t (two-clauses 1 2 3))

;; A row named `test_case_lambda_empty_clause_list` claimed `(case-lambda)` is
;; an error. It is not: it expands to `(lambda args (error …))`, so it *is* a
;; procedure, and every call to it raises. That row passed because it ran
;; without the import, so what it actually held was that `case-lambda` is
;; unbound until `(scheme case-lambda)` is imported — a property of the import
;; set, which this file cannot state because it imports the library at the top.
;; Both halves now live in `import_set_is_enforced.rs`.

;; ── Recursion and tail position ─────────────────────────────────────────────

;; A self-call in a clause's tail position, reached *through* the zero-argument
;; clause: `(countdown)` selects `(() (countdown 100))`, which tail-calls the
;; one-argument clause 100 times.
(define countdown
  (case-lambda (() (countdown 100))
               ((n) (if (= n 0) 'done (countdown (- n 1))))))
(test-equal "tail self-call, entered through the zero-argument clause" 'done
  (countdown))

;; Tail calls that cross clauses, at a depth where a non-tail implementation
;; would be doing 1000 nested frames rather than looping.
(define mutual-rec
  (case-lambda ((n) (if (= n 0) 'done (mutual-rec n 0)))
               ((n acc) (if (= n 0) acc (mutual-rec (- n 1) (+ acc 1))))))
(test-equal "tail calls across clauses" 1000 (mutual-rec 1000))

;; Non-tail recursion through the one-argument clause — kept because it is the
;; shape most user code has, not because it says anything about tail position.
(define fib
  (case-lambda (() (fib 10))
               ((n) (if (<= n 1) n (+ (fib (- n 1)) (fib (- n 2)))))))
(test-equal "non-tail recursion, one-argument clause" 13 (fib 7))
(test-equal "…and the zero-argument clause is reachable" 55 (fib))

(define my-list
  (case-lambda (() '()) ((x) (list x)) ((x y) (list x y)) (args args)))
(test-equal "a clause returning a list at every arity" '(() (1) (1 2) (1 2 3 4 5))
  (list (my-list) (my-list 1) (my-list 1 2) (my-list 1 2 3 4 5)))

(test-end)
