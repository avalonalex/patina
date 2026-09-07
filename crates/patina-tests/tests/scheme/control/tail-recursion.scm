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
;; call, at any of the depths used below — the largest here is 10 000.
;;
;; So what each row actually pins is that the form evaluates *correctly* when
;; its last expression recurses, which is worth having and is what the row
;; names say. Proper tail calls in the R7RS 3.5 sense — constant space — need
;; an instrument these assertions do not have, because the failure they would
;; look for is memory growth and not a wrong answer or a crash.
;;
;; Every row therefore keeps its original depth. Raising the numbers would buy
;; nothing: there is no depth at which passing means more than it does now.
;;
;; Both backends and Gauche agree on all 36 rows; chibi agrees on all 36 too.
;;
;; **Names are unique per row on purpose.** In the `.rs` form each row was its
;; own program with its own top level, and a dozen of them defined `countdown`.
;; Here they share one, so a later `define` would quietly replace an earlier
;; row's helper. Two rows also defined `even?` and `odd?`, which at this file's
;; top level would shadow the standard procedures for every row after them.

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
  (let ((threshold 100))
    (and (> n 0)
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
;;
;; Named `-via-if` and `ping`/`pong` rather than `even?`/`odd?`: these are
;; top-level, and shadowing the standard predicates would leak into every row
;; below them.

(define (even-via-if? n) (if (= n 0) #t (odd-via-if? (- n 1))))
(define (odd-via-if? n) (if (= n 0) #f (even-via-if? (- n 1))))
(test-equal "mutual recursion through if" #t
  (even-via-if? 5000))

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

(test-end)
