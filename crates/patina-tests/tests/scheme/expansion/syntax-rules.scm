;; Well-known macros written with `syntax-rules` — the textbook forms a
;; hygienic macro system is expected to handle on day one: control flow,
;; binding forms, mutation, short-circuit logic, loops, and data carried
;; through pattern variables.
;;
;; Migrated from `crates/patina-tests/tests/compliance/macros_advanced.rs`
;; (#193), which is deleted. That module's 60 tests went by subject:
;;
;;   - the classic macros, here;
;;   - the identifiers a template introduces across recursive expansions —
;;     distinct parameters and definitions, and the generated macros that must
;;     still reach them — to `introduced-definitions.scm`;
;;   - literal matching and identifier identity in a macro that writes a
;;     macro, to `syntax-rules-literals.scm`;
;;   - SRFI 46's declared ellipsis and the `(... ...)` escape in a generated
;;     macro, to `ellipsis.scm`;
;;   - the two top-level hygiene shapes and one macro-writing-macro shape, to
;;     `hygiene.scm`, beside the rows they are variants of.
;;
;; Each Rust test was a program of its own. Here they share one top level, so
;; every macro and every global has a name no other row uses, and each row's
;; side effects happen inside the row. Where the Rust test defined the same
;; macro twice (`my-when` and `my-unless`, which the nesting row reuses), this
;; file defines it once and says so.
;;
;; ── Measured 2026-09-11 (chibi 0.12, Gauche via `gosh -r7`) ─────────────────
;;
;;   patina VM / tree-walker   30 pass
;;   chibi                     29 pass, 1 skipped (the Patina-scoped row)
;;   Gauche                    29 pass, 1 skipped (the same)
;;
;; No oracle differs, and the register has no entry for this file.

(import (scheme base) (srfi 64))

(test-begin "syntax-rules")

;; ── Control flow ────────────────────────────────────────────────────────────

(define-syntax my-when
  (syntax-rules ()
    ((my-when test body ...)
     (if test (begin body ...)))))

(define-syntax my-unless
  (syntax-rules ()
    ((my-unless test body ...)
     (if (not test) (begin body ...)))))

(define when-result 0)
(test-equal "my-when runs its body in order when the test is true" 15
  (begin
    (my-when (> 5 3)
      (set! when-result 10)
      (set! when-result (+ when-result 5)))
    when-result))

(define unless-result 0)
(test-equal "my-unless runs its body when the test is false" 20
  (begin
    (my-unless (< 5 3)
      (set! unless-result 20))
    unless-result))

;; A macro that recurses on its own clauses, with `else` as a literal.
(define-syntax my-cond
  (syntax-rules (else)
    ((my-cond (else result))
     result)
    ((my-cond (test result))
     (if test result))
    ((my-cond (test result) clause ...)
     (if test result (my-cond clause ...)))))

(test-equal "a recursive cond with an else literal" 'greater
  (my-cond
    ((< 5 3) 'less)
    ((> 5 3) 'greater)
    (else 'equal)))

;; ── Binding forms ───────────────────────────────────────────────────────────

;; `let` in terms of `lambda`, the classic first macro.
(define-syntax my-let
  (syntax-rules ()
    ((my-let ((var val) ...) body ...)
     ((lambda (var ...) body ...) val ...))))

(test-equal "let written as a lambda application" 30
  (my-let ((x 10) (y 20))
    (+ x y)))

(define-syntax named-let
  (syntax-rules ()
    ((named-let name ((var val) ...) body ...)
     (letrec ((name (lambda (var ...) body ...)))
       (name val ...)))))

(test-equal "a named let for recursion" 120
  (named-let loop ((n 5) (acc 1))
    (if (= n 0)
        acc
        (loop (- n 1) (* acc n)))))

;; A simplified `let-values`: R7RS has one built in, and this is the macro
;; version of the idea.
(define-syntax simple-let-values
  (syntax-rules ()
    ((simple-let-values ((var ...)) expr body ...)
     (call-with-values
       (lambda () expr)
       (lambda (var ...) body ...)))))

(test-equal "a let-values written with call-with-values" 30
  (simple-let-values ((a b))
    (values 10 20)
    (+ a b)))

;; A pattern that destructures nested list structure.
(define-syntax let-pair
  (syntax-rules ()
    ((let-pair ((a b) pair-expr) body ...)
     (let ((temp pair-expr))
       (let ((a (car temp))
             (b (car (cdr temp))))
         body ...)))))

(test-equal "a pattern that matches nested structure" 30
  (let-pair ((x y) '(10 20))
    (+ x y)))

;; ── Mutation ────────────────────────────────────────────────────────────────

;; The Lisp `push!`, assigning to a global the caller names.
(define-syntax push!
  (syntax-rules ()
    ((push! item lst)
     (set! lst (cons item lst)))))

(define pushed-list '(2 3))
(test-equal "push! assigns to the variable it is given" '(1 2 3)
  (begin (push! 1 pushed-list) pushed-list))

;; Two rules, chosen by arity.
(define-syntax inc!
  (syntax-rules ()
    ((inc! var)
     (set! var (+ var 1)))
    ((inc! var delta)
     (set! var (+ var delta)))))

(define inc-x 10)
(test-equal "inc! with and without a delta" 16
  (begin (inc! inc-x) (inc! inc-x 5) inc-x))

;; ── Logic ───────────────────────────────────────────────────────────────────

(define-syntax my-and
  (syntax-rules ()
    ((my-and) #t)
    ((my-and test) test)
    ((my-and test1 test2 ...)
     (if test1 (my-and test2 ...) #f))))

(test-equal "a short-circuit and, including the empty case" '(#t #f #t)
  (list
    (my-and (> 5 3) (< 2 4))
    (my-and (> 5 3) (> 2 4))
    (my-and)))

(define-syntax my-or
  (syntax-rules ()
    ((my-or) #f)
    ((my-or test) test)
    ((my-or test1 test2 ...)
     (let ((temp test1))
       (if temp temp (my-or test2 ...))))))

(test-equal "a short-circuit or, including the empty case" '(#t #f #f)
  (list
    (my-or (< 5 3) (> 2 4) (= 1 1))
    (my-or (< 5 3) (> 2 4))
    (my-or)))

;; Returns its first value but evaluates everything: `(+ begin0-x 10)` is
;; taken while `begin0-x` is still 1.
(define-syntax begin0
  (syntax-rules ()
    ((begin0 first rest ...)
     (let ((temp first))
       rest ...
       temp))))

(define begin0-x 1)
(test-equal "begin0 returns its first value after running the rest" 11
  (begin0
    (+ begin0-x 10)
    (set! begin0-x 20)
    (set! begin0-x 30)))

;; ── Loops ───────────────────────────────────────────────────────────────────

(define-syntax dotimes
  (syntax-rules ()
    ((dotimes (var count) body ...)
     (letrec ((loop (lambda (var)
                      (if (< var count)
                          (begin
                            body ...
                            (loop (+ var 1)))))))
       (loop 0)))))

(define dotimes-sum 0)
(test-equal "dotimes counts from zero" 10   ; 0 + 1 + 2 + 3 + 4
  (begin
    (dotimes (i 5)
      (set! dotimes-sum (+ dotimes-sum i)))
    dotimes-sum))

(define-syntax while
  (syntax-rules ()
    ((while test body ...)
     (letrec ((loop (lambda ()
                      (if test
                          (begin
                            body ...
                            (loop))))))
       (loop)))))

(define while-n 5)
(define while-result 1)
(test-equal "a while loop" 120
  (begin
    (while (> while-n 0)
      (set! while-result (* while-result while-n))
      (set! while-n (- while-n 1)))
    while-result))

;; ── Ellipsis patterns ───────────────────────────────────────────────────────

;; `list*`: like `cons`, but with any number of leading elements.
(define-syntax list*
  (syntax-rules ()
    ((list* last)
     last)
    ((list* first rest ...)
     (cons first (list* rest ...)))))

(test-equal "list* conses leading elements onto the last" '(1 2 3 4 5)
  (list* 1 2 3 '(4 5)))

;; A nested ellipsis flattened by `expr ... ...` in the template. The
;; assignments run in order: 0, 1, 2, 12, 32.
(define-syntax multi-begin
  (syntax-rules ()
    ((multi-begin (expr ...) ...)
     (begin expr ... ...))))

(define multi-x 0)
(test-equal "a nested ellipsis flattened by two ellipses" 32
  (begin
    (multi-begin
      ((set! multi-x 1) (set! multi-x (+ multi-x 1)))
      ((set! multi-x (+ multi-x 10)) (set! multi-x (+ multi-x 20))))
    multi-x))

;; The dummy `times` are spliced in front and evaluated for nothing, so the
;; expression still runs once.
(define-syntax repeat
  (syntax-rules ()
    ((repeat (times ...) expr)
     (begin times ... expr))))

(define repeat-x 0)
(test-equal "spliced dummies do not repeat the expression" 1
  (begin
    (repeat (#f #f #f) (set! repeat-x (+ repeat-x 1)))
    repeat-x))

(define-syntax maybe-begin
  (syntax-rules ()
    ((maybe-begin body ...)
     (begin body ...))))

(test-equal "an ellipsis matching one item" 42 (maybe-begin 42))

;; And zero: `(maybe-begin)` expands to `(begin)`. In a body that is an empty
;; sequence of definitions, which every implementation here accepts.
(test-equal "an ellipsis matching zero items" 'untouched
  (let ((x 'untouched))
    (maybe-begin)
    x))

;; The Rust original asserted `(maybe-begin)`'s value as an *expression* —
;; `(42 #<unspecified>)` for `(list (maybe-begin 42) (maybe-begin))`. `(begin)`
;; with no expressions is not an R7RS expression, so that value is Patina's
;; choice alone: the unspecified value, as `(if #f #f)` gives. Kept as an
;; argument to `list`, as the original had it: as the whole of a body, which
;; is what `test-equal`'s thunk would make of a bare `(maybe-begin)`, Patina
;; refuses an empty body instead.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an empty begin in expression position is the unspecified value"
  (list 42 (if #f #f))
  (list (maybe-begin 42) (maybe-begin)))

;; Discards everything it is given, including things that are not expressions.
(define-syntax comment
  (syntax-rules ()
    ((comment anything ...)
     (begin))))

(test-equal "a macro that discards its arguments" 10
  (let ((comment-x 10))
    (comment
      (set! comment-x 999)
      (set! comment-x 'should-not-execute)
      this is all ignored)
    comment-x))

;; ── Recursion and nesting ───────────────────────────────────────────────────

(define-syntax build-list
  (syntax-rules ()
    ((build-list)
     '())
    ((build-list x)
     (cons x '()))
    ((build-list x y ...)
     (cons x (build-list y ...)))))

(test-equal "a macro that expands into itself" '(1 2 3 4 5)
  (build-list 1 2 3 4 5))

;; Three levels: `safe-div` expands to a `my-unless` whose body is a
;; `my-when`. Both are the definitions at the top of this file; the Rust test
;; repeated them verbatim.
(define-syntax safe-div
  (syntax-rules ()
    ((safe-div a b default)
     (my-unless (= b 0)
       (my-when (> a 0)
         (/ a b))))))

(test-equal "three levels of macro nesting" 5 (safe-div 10 2 0))

(define-syntax trace
  (syntax-rules ()
    ((trace expr)
     (let ((result expr))
       result))))

(test-equal "a macro that binds and returns its expression's value" 6
  (trace (+ 1 2 3)))

;; ── Data through pattern variables ──────────────────────────────────────────

;; Quoted symbols in a template come out as the symbols written, so the
;; answers compare equal to the caller's `'ok`.
(define-syntax my-assert
  (syntax-rules ()
    ((my-assert test)
     (if (not test)
         'assertion-failed
         'ok))))

(test-equal "a template's quoted symbols are the symbols written" '(ok ok)
  (list
    (my-assert (= 2 2))
    (my-assert (> 5 3))))

;; A literal vector passed through a pattern variable compares equal to a
;; quoted one. Regression: vectors from pattern variables once had their
;; symbols marked with scopes, so the comparison failed. The second row is
;; the shape of chibi's own `test` macro, which is where it was found.
(define-syntax check-vector
  (syntax-rules ()
    ((check-vector expected actual)
     (equal? expected actual))))

(test-assert "a vector literal through a pattern variable"
  (check-vector #(a b c) '#(a b c)))

(define-syntax check-equal
  (syntax-rules ()
    ((check-equal expected expr)
     (equal? expected expr))))

(test-assert "and against an explicit quote"
  (check-equal #(a b c) (quote #(a b c))))

;; Quasiquote inside a macro argument. Regression: after expansion,
;; `unquote-splicing` arrived as an identifier rather than a symbol, and the
;; quasiquote evaluator had to accept both.
(test-assert "unquote-splicing inside a macro argument"
  (check-equal '(a 3 4 5 6 b) `(a ,(+ 1 2) ,@(map abs '(4 -5 6)) b)))

;; Regression: symbols in quoted data from a pattern variable became
;; identifiers with empty scopes, and stopped comparing equal to what
;; quasiquote built.
(test-assert "a quoted list through a pattern variable equals a quasiquoted one"
  (check-equal '(list 3 4) `(list ,(+ 1 2) 4)))

;; A literal keyword in a pattern, the way `cond` uses `=>`.
(define-syntax arrow-if
  (syntax-rules (=>)
    ((arrow-if test => then-expr)
     (if test then-expr #f))))

(test-equal "a literal keyword in a pattern" 42 (arrow-if (> 5 3) => 42))

(test-end)
