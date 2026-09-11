;; Tail positions in every special form that has one.
;;
;; Migrated whole from `crates/patina-tests/tests/tail_recursion.rs` (#193
;; Phase 1). Every row was one `assert_program_eval_to`, so nothing stayed
;; behind.
;;
;; ## What these rows do and do not show
;;
;; The `.rs` file's header said they "verify that all special forms correctly
;; handle tail positions and enable deep recursion without stack overflow".
;; The first half is right. **The second half is not demonstrated by anything
;; here**, and measuring is what shows it: on both backends a *non*-tail
;; recursion — `(define (s n) (if (= n 0) 0 (+ n (s (- n 1)))))` — returns
;; correctly at 10 000, at 200 000 and at 1 000 000 (measured 2026-09-07).
;; Both backends allocate frames on the heap rather than the native stack, so
;; surviving a deep recursion does not distinguish a tail call from any other
;; call, at any of the depths used below — the largest here is 100 000.
;;
;; So what each row actually pins is that the form evaluates *correctly* when
;; its last expression recurses, which is worth having and is what the row
;; names say. Constant space in the R7RS 3.5 sense is a different claim, and
;; the failure it would look for is memory growth rather than a wrong answer.
;;
;; **That property is not untested in this repo, only untested here.**
;; `vm_callprimitive.rs::tail_deopt_runs_deep_mutual_recursion` pins the
;; semantics at 100 000 through a rebound primitive, and PRD TRACK_P §P8.2
;; records the measurement itself: 500 000 iterations at **5.49 MB** max RSS
;; after the fix against **109 MB** before. What is missing is an automated
;; in-suite check of the space property, not the property's verification.
;;
;; Every row therefore keeps its original depth. Raising the numbers within
;; reach of a `test-equal` would buy nothing — the discrimination is by memory,
;; and the retained frames cost on the order of a hundred bytes each, so a
;; depth where exhaustion separates the two is far past anything belonging in
;; a unit suite.
;;
;; Both backends and Gauche agree on all 38 rows; chibi agrees on all 38 too.
;;
;; **Helper names are unique per row on purpose.** In the `.rs` form each row
;; was its own program with its own top level; seven of them defined
;; `countdown` there. Here they share one top level, so a later `define` would
;; quietly replace an earlier row's helper.
;;
;; The exception is `even?`/`odd?`, which keep their names deliberately — see
;; the note on that row. Two rows defined them, but only one at the top level;
;; the other binds them in a `letrec`, where they cannot leak and are left
;; exactly as they were.

(import (scheme base) (srfi 64))

(test-begin "tail-recursion")

;; ── if: both branches ───────────────────────────────────────────────────────

(define (countdown-if-then n)
  (if (= n 0) 'done (countdown-if-then (- n 1))))
(test-equal "the then-branch of if is a tail position" 'done
  (countdown-if-then 1000))

(define (countup-if-else n limit)
  (if (= n limit) n (countup-if-else (+ n 1) limit)))
(test-equal "the else-branch of if is a tail position" 1000
  (countup-if-else 0 1000))

(define (classify-nested-if n)
  (if (= n 0)
      'zero
      (if (< n 0) (classify-nested-if (+ n 1)) (classify-nested-if (- n 1)))))
(test-equal "the innermost branch of nested ifs is a tail position" 'zero
  (classify-nested-if 500))

;; ── begin: the last expression ──────────────────────────────────────────────

(define (countdown-begin n)
  (begin
    (+ n 1)   ; not a tail position
    (* n 2)   ; not a tail position
    (if (= n 0) 'done (countdown-begin (- n 1)))))
(test-equal "only begin's last expression is a tail position" 'done
  (countdown-begin 1000))

;; ── cond: clause bodies ─────────────────────────────────────────────────────

(define (collatz n)
  (cond ((= n 1) 1)
        ((= (remainder n 2) 0) (collatz (/ n 2)))
        (else (collatz (+ (* 3 n) 1)))))
(test-equal "a cond clause body is a tail position" 1
  (collatz 27))   ; 111 steps to reach 1

(define (categorize-cond n)
  (cond ((< n 0) (categorize-cond (+ n 1000)))
        ((= n 0) 'zero)
        ((< n 100) (categorize-cond (- n 10)))
        (else (categorize-cond (- n 100)))))
(test-equal "every cond clause body is a tail position, else included" 'zero
  (categorize-cond 1500))

;; ── and / or: the last expression ───────────────────────────────────────────

(define (all-positive? lst)
  (and (pair? lst)
       (> (car lst) 0)
       (if (null? (cdr lst)) #t (all-positive? (cdr lst)))))
(test-equal "and's last expression is a tail position" #t
  (all-positive? '(1 2 3 4 5 6 7 8 9 10)))

(define (countdown-and n)
  (and (> n 0) (countdown-and (- n 1))))
(test-equal "and recurses to its own short circuit" #f
  (countdown-and 1000))

(define (find-zero? lst)
  (or (null? lst) (= (car lst) 0) (find-zero? (cdr lst))))
(test-equal "or's last expression is a tail position" #t
  (find-zero? '(1 2 3 4 5 0 7 8)))

(define (search-value n target)
  (or (= n target) (> n 1000) (search-value (+ n 1) target)))
(test-equal "or recurses until a clause is true" #t
  (search-value 0 500))

;; ── let, let*: the body ─────────────────────────────────────────────────────

(define (factorial-let n)
  (let ((acc 1))
    (define (iter n acc) (if (= n 0) acc (iter (- n 1) (* n acc))))
    (iter n acc)))
(test-equal "a let body is a tail position" 3628800
  (factorial-let 10))

(define (sum-range start end)
  (let ((sum 0))
    (let ((current start))
      (define (iter cur acc) (if (> cur end) acc (iter (+ cur 1) (+ acc cur))))
      (iter current sum))))
(test-equal "the innermost let body is the tail position" 5050
  (sum-range 1 100))

(define (gcd-iter a b)
  (let* ((r (remainder a b)))
    (if (= r 0) b (gcd-iter b r))))
(test-equal "a let* body is a tail position" 21
  (gcd-iter 1071 462))

(define (power-let-star base exp)
  (let* ((result 1)
         (counter exp))
    (define (iter n acc) (if (= n 0) acc (iter (- n 1) (* acc base))))
    (iter counter result)))
(test-equal "a let* body with sequential bindings is a tail position" 1024
  (power-let-star 2 10))

;; ── letrec, letrec*: the body ───────────────────────────────────────────────

(test-equal "a letrec body is a tail position" 'done
  (letrec ((countdown (lambda (n) (if (= n 0) 'done (countdown (- n 1))))))
    (countdown 1000)))

;; `even?` / `odd?` are shadowed here deliberately, and only inside the letrec
;; — the point is that letrec's bindings see each other. The mutual-recursion
;; rows further down use distinct names because theirs are top-level.
(test-equal "letrec bindings can recurse through each other in tail position" #t
  (letrec ((even? (lambda (n) (if (= n 0) #t (odd? (- n 1)))))
           (odd? (lambda (n) (if (= n 0) #f (even? (- n 1))))))
    (even? 5000)))

(test-equal "a letrec* body is a tail position" 'done
  (letrec* ((countdown (lambda (n) (if (= n 0) 'done (countdown (- n 1))))))
    (countdown 1000)))

;; ── let-values, let*-values: the body ───────────────────────────────────────

(define (countdown-let-values n acc)
  (let-values (((a b) (values n acc)))
    (if (= a 0) b (countdown-let-values (- a 1) (+ b 1)))))
(test-equal "a let-values body is a tail position" 1000
  (countdown-let-values 1000 0))

(define (fib n)
  (define (fib-iter a b count)
    (let-values (((x y) (values a b)))
      (if (= count 0) x (fib-iter y (+ x y) (- count 1)))))
  (fib-iter 0 1 n))
(test-equal "let-values carries an accumulator pair through the recursion"
  354224848179261915075
  (fib 100))

(define (countdown-let-star-values n)
  (let*-values (((x) (values n)))
    (if (= x 0) 'done (countdown-let-star-values (- x 1)))))
(test-equal "a let*-values body is a tail position" 'done
  (countdown-let-star-values 1000))

;; ── case: clause bodies ─────────────────────────────────────────────────────

(define (classify-case n)
  (case (remainder n 3)
    ((0) (if (= n 0) 'zero (classify-case (- n 3))))
    ((1) (if (= n 1) 'one (classify-case (- n 3))))
    ((2) (if (= n 2) 'two (classify-case (- n 3))))))
(test-equal "a case clause body is a tail position" 'zero
  (classify-case 3000))

(define (countdown-case n)
  (case n ((0) 'done) (else (countdown-case (- n 1)))))
(test-equal "a case else clause is a tail position" 'done
  (countdown-case 1000))

;; ── do: the exit expression ─────────────────────────────────────────────────

(define (helper-do n) (if (= n 0) 'done (helper-do (- n 1))))
(test-equal "a do loop's exit expression is a tail position" 'done
  (do ((i 0 (+ i 1)))
      ((= i 1) (helper-do 1000))))

(define (factorial-iter-do n acc)
  (if (= n 0) acc (factorial-iter-do (- n 1) (* n acc))))
(test-equal "a do loop can exit into a tail call" 3628800
  (do ((x 10))
      ((= x 10) (factorial-iter-do x 1))))

;; ── Several forms at once ───────────────────────────────────────────────────

(define (complex-recurse n)
  (if (< n 0)
      (complex-recurse (+ n 100))
      (cond ((= n 0) 'done)
            ((< n 50) (complex-recurse (- n 1)))
            (else (complex-recurse (- n 10))))))
(test-equal "if and cond nested, both in tail position" 'done
  (complex-recurse 500))

(define (validate-and-process n)
  (let ((positive? (> n 0)))
    (and positive?
         (or (= n 1) (validate-and-process (- n 1))))))
(test-equal "and and or inside a let body, all in tail position" #t
  (validate-and-process 500))

(define (nested-countdown n)
  (begin
    (let ((x n))
      (if (> x 0)
          (begin (cond ((= x 1) 'done) (else (nested-countdown (- x 1)))))
          'done))))
(test-equal "begin, let, if and cond nested throughout" 'done
  (nested-countdown 1000))

(test-equal "letrec with case in tail position" 'zero
  (letrec ((classify (lambda (n)
                       (case (remainder n 2)
                         ((0) (if (= n 0) 'zero (classify (- n 2))))
                         ((1) (if (= n 1) 'one (classify (- n 2))))))))
    (classify 1000)))

(define (foobar n)
  (let-values (((x y) (values n 0)))
    (case (remainder x 3)
      ((0) (if (= x 0) 'done (foobar (- x 3))))
      (else (foobar (- x 1))))))
(test-equal "let-values with case in tail position" 'done
  (foobar 900))

;; ── Mutual recursion at the top level ───────────────────────────────────────

;; `even?` and `odd?` keep their names, and that is the row rather than an
;; oversight: both are **registry primitives**
;; (`patina-primitives/src/primitives/arithmetic/predicates.rs`), so defining
;; them at the top level forces the VM to deoptimise the inlined
;; `CallPrimitive` sites and then tail-call the Scheme replacements 5 000
;; times. That is the shape PRD TRACK_P P8.2 exists for, and the one
;; `vm_callprimitive.rs::tail_deopt_runs_deep_mutual_recursion` pins with
;; `car` at 100 000. Renaming them to ordinary globals — which an earlier
;; draft of this file did — silently drops that path while every row still
;; passes.
;;
;; Shadowing them here is safe because nothing below uses either name; the
;; library's own references resolve inside the library, not through this top
;; level. Anything added after this point must not call `even?` or `odd?`.
(define (even? n) (if (= n 0) #t (odd? (- n 1))))
(define (odd? n) (if (= n 0) #f (even? (- n 1))))
(test-equal "mutual recursion through if, over redefined primitives" #t
  (even? 5000))

;; `ping`/`pong` are the original names, not a rename — nothing here shadows.
(define (ping n) (cond ((= n 0) 'done) (else (pong (- n 1)))))
(define (pong n) (cond ((= n 0) 'done) (else (ping (- n 1)))))
(test-equal "mutual recursion through cond" 'done
  (ping 5000))

;; ── The deepest rows in the file ────────────────────────────────────────────
;;
;; 10 000 and 5 000 iterations. Kept at the original depths — see the header on
;; why a larger number would not say more.

(define (countdown-if-stress n)
  (if (= n 0) 'done (countdown-if-stress (- n 1))))
(test-equal "if, ten thousand deep" 'done
  (countdown-if-stress 10000))

(define (countdown-cond-stress n)
  (cond ((= n 0) 'done) (else (countdown-cond-stress (- n 1)))))
(test-equal "cond, ten thousand deep" 'done
  (countdown-cond-stress 10000))

(define (countdown-let-stress n)
  (let ((next (- n 1)))
    (if (= n 0) 'done (countdown-let-stress next))))
(test-equal "let, ten thousand deep" 'done
  (countdown-let-stress 10000))

(define (countdown-or-stress n)
  (or (= n 0) (countdown-or-stress (- n 1))))
(test-equal "or, five thousand deep" #t
  (countdown-or-stress 5000))

(define (test-case-stress n)
  (case (remainder n 2)
    ((0) (if (= n 0) 'even-zero (test-case-stress (- n 2))))
    ((1) (if (= n 1) 'odd-one (test-case-stress (- n 2))))))
(test-equal "case, five thousand deep" 'even-zero
  (test-case-stress 5000))

;; The last two came from `compliance/numbers.rs` (#193), where they sat among
;; the numeric rows. The header's caveat applies to them as to the rest: they
;; show a correct answer at depth, not constant space.
(define (countdown-deep n) (if (= n 0) 'done (countdown-deep (- n 1))))
(test-equal "a self tail call 100 000 deep" 'done (countdown-deep 100000))
(test-equal "mutual recursion 10 000 deep" #t
  (letrec ((ev? (lambda (n) (if (= n 0) #t (od? (- n 1)))))
           (od? (lambda (n) (if (= n 0) #f (ev? (- n 1))))))
    (ev? 10000)))

(test-end)
