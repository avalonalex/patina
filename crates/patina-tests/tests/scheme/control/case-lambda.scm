;; `case-lambda` — a procedure that dispatches on how many arguments it got.
;;
;; R7RS §4.2.9, provided by `(scheme case-lambda)`, which Patina implements as
;; a macro in Scheme rather than a special form. Migrated from
;; `crates/patina-tests/tests/case_lambda.rs` (#193 Phase 1); the rows are the
;; same programs and the same expected values.
;;
;; The interesting rows are the arity-selection ones. A clause list is tried in
;; order and the first arity that fits wins, so `(() 'zero)` before `(args …)`
;; and `((x y) …)` before `((x y . z) …)` are what distinguish a working
;; dispatch from one that always falls through to the variadic clause — which
;; is why the tables below call one procedure at every arity rather than
;; testing each clause alone.

(import (scheme base) (scheme case-lambda) (srfi 64))

(test-begin "case-lambda")

;; ── Arity selection ─────────────────────────────────────────────────────────

(test-equal "zero arguments" 'zero
  (let () (define f (case-lambda (() 'zero))) (f)))

(test-equal "one argument" 42
  (let () (define f (case-lambda ((x) x))) (f 42)))

(test-equal "two arguments" '(1 . 2)
  (let () (define f (case-lambda ((x y) (cons x y)))) (f 1 2)))

;; The first clause whose arity fits wins, so calling one procedure at every
;; arity is what shows dispatch working rather than one clause matching always.
(test-equal "several clauses, called at each arity" '(zero 1 (1 . 2))
  (let ()
    (define f (case-lambda (() 'zero) ((x) x) ((x y) (cons x y))))
    (list (f) (f 1) (f 1 2))))

;; ── Variadic clauses ────────────────────────────────────────────────────────

(test-equal "a lone variadic clause collects its arguments" '(1 2 3)
  (let () (define f (case-lambda (args args))) (f 1 2 3)))

(test-equal "…and accepts none" '()
  (let () (define f (case-lambda (args args))) (f)))

(test-equal "fixed head plus a rest tail" '(1 2 3)
  (let () (define f (case-lambda ((x . rest) (cons x rest)))) (f 1 2 3)))

(test-equal "fixed clauses before a variadic one"
  '(zero (one 1) (two 1 2) (more 1 2 (3)) (more 1 2 (3 4)))
  (let ()
    (define f
      (case-lambda (() 'zero)
                   ((x) (list 'one x))
                   ((x y) (list 'two x y))
                   ((x y . z) (list 'more x y z))))
    (list (f) (f 1) (f 1 2) (f 1 2 3) (f 1 2 3 4))))

;; ── R7RS's own examples, as chibi's suite states them ───────────────────────

(test-equal "any-arity" '(zero 1 (1 . 2) (1 2 3) (many 1 2 3 4))
  (let ()
    (define any-arity
      (case-lambda (() 'zero)
                   ((x) x)
                   ((x y) (cons x y))
                   ((x y z) (list x y z))
                   (args (cons 'many args))))
    (list (any-arity) (any-arity 1) (any-arity 1 2)
          (any-arity 1 2 3) (any-arity 1 2 3 4))))

(test-equal "rest-arity" '((zero) (one 1) (two 1 2) (more 1 2 (3)) (more 1 2 (3 4)))
  (let ()
    (define rest-arity
      (case-lambda (() '(zero))
                   ((x) (list 'one x))
                   ((x y) (list 'two x y))
                   ((x y . z) (list 'more x y z))))
    (list (rest-arity) (rest-arity 1) (rest-arity 1 2)
          (rest-arity 1 2 3) (rest-arity 1 2 3 4))))

;; ── Closure behaviour ───────────────────────────────────────────────────────

;; Every clause closes over the same environment, not one apiece.
(test-equal "clauses share the enclosing environment" '(10 15)
  (let ()
    (define x 10)
    (define f (case-lambda (() x) ((y) (+ x y))))
    (list (f) (f 5))))

;; And over the same mutable state: the counter's two clauses read and write
;; one `n`, so `(0 5 8 8)` rather than each clause getting its own binding.
(test-equal "clauses share mutable state" '(0 5 8 8)
  (let ()
    (define (make-counter n)
      (case-lambda (() n) ((x) (set! n (+ n x)) n)))
    (define c (make-counter 0))
    (list (c) (c 5) (c 3) (c))))

;; ── As an ordinary value ────────────────────────────────────────────────────

(test-equal "passed as an argument" '(1 3)
  (let ()
    (define (apply-twice f) (list (f 1) (f 1 2)))
    (apply-twice (case-lambda ((x) (* x x)) ((x y) (+ x y))))))

(test-equal "returned from a procedure" '(8 11)
  (let ()
    (define (make-adder n)
      (case-lambda ((x) (+ n x)) ((x y) (+ n x y))))
    (define add5 (make-adder 5))
    (list (add5 3) (add5 2 4))))

(test-equal "the library's binding is what is being used" 'imported
  (let () (define f (case-lambda (() 'imported) ((x) x))) (f)))

;; ── Errors ──────────────────────────────────────────────────────────────────

(test-error "no clause matches the call" #t
  (let ()
    (define f (case-lambda ((x) x) ((x y) (cons x y))))
    (f 1 2 3)))

;; The Rust file had a row named `test_case_lambda_empty_clause_list`, asserting
;; `(case-lambda)` is an error. It is not: with the library imported it returns
;; a procedure that matches no call. That row passed because it ran *without*
;; the import, so what it actually asserted was that `case-lambda` is unbound
;; until `(scheme case-lambda)` is imported — a property of the import set, not
;; of empty clause lists, and one this file cannot state because it imports the
;; library at the top. Moved to `import_set_is_enforced.rs`, where it belongs
;; and where its name can be true. Found by migrating: writing the row in
;; Scheme is what forced the question of what it asserted.

;; ── Tail position ───────────────────────────────────────────────────────────

;; A self-call in a clause's tail position must not grow the stack; the counts
;; are large enough that a non-tail implementation overflows rather than
;; returning a wrong answer.
(test-equal "self tail call, one clause to another" 'done
  (let ()
    (define countdown
      (case-lambda (() (countdown 100))
                   ((n) (if (= n 0) 'done (countdown (- n 1))))))
    (countdown)))

(test-equal "tail calls across clauses" 1000
  (let ()
    (define mutual-rec
      (case-lambda ((n) (if (= n 0) 'done (mutual-rec n 0)))
                   ((n acc) (if (= n 0) acc (mutual-rec (- n 1) (+ acc 1))))))
    (mutual-rec 1000)))

(test-equal "recursion through the zero-argument clause" 13
  (let ()
    (define fib
      (case-lambda (() (fib 10))
                   ((n) (if (<= n 1) n (+ (fib (- n 1)) (fib (- n 2)))))))
    (fib 7)))

(test-equal "a clause returning a list at every arity" '(() (1) (1 2) (1 2 3 4 5))
  (let ()
    (define my-list
      (case-lambda (() '()) ((x) (list x)) ((x y) (list x y)) (args args)))
    (list (my-list) (my-list 1) (my-list 1 2) (my-list 1 2 3 4 5))))

(test-end)
