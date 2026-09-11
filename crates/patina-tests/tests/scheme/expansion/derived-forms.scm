;; The derived expression types — R7RS §4.2: `cond`, `case`, `and`, `or`,
;; `when`, `unless`, `let`, `let*`, `letrec`, `let-values`, `let*-values` and
;; `do` — plus the few `syntax-rules` macros written over them.
;;
;; Migrated from `crates/patina-tests/tests/compliance/derived.rs` (#193),
;; which is deleted. Its rows were adapted from chibi's `r7rs-tests.scm`, many
;; of them the report's own examples. Patina defines every one of these forms
;; as a macro in `lib/scheme/base/*.scm`, not as a special form, so these rows
;; are tests of those macros as much as of the forms.
;;
;; `let-values` has a file of its own, `expansion/let-values.scm`, holding one
;; row about a body definition. The rows here are the ordinary shapes, kept
;; with the other binding constructs they were written beside.
;;
;; **Unspecified values.** Eight Rust assertions compared a result against
;; `#<unspecified>`: a `when` whose test fails, a `case` that matches nothing,
;; a `do` with no result expressions, and so on. R7RS leaves every one of those
;; values unspecified, so what they print as is Patina's alone, and those rows
;; are scoped to Patina and compare the written form, as the Rust file did.
;; Where a portable claim hides inside one — a matched `case` clause with an
;; empty body still stops the dispatch — it has a row of its own as well.

(import (scheme base) (scheme cxr) (scheme write) (srfi 64))

(define (written x)
  (let ((p (open-output-string))) (write x p) (get-output-string p)))

(test-begin "derived-forms")

;; ── 4.2.1 cond ─────────────────────────────────────────────────────────────

(test-equal "cond takes the first true clause" 'greater
  (cond ((> 3 2) 'greater) ((< 3 2) 'less)))
(test-equal "cond falls through to else" 'equal
  (cond ((> 3 3) 'greater) ((< 3 3) 'less) (else 'equal)))
(test-equal "cond's => passes the test's value" 2
  (cond ((assv 'b '((a 1) (b 2))) => cadr) (else #f)))
(test-equal "a false => clause is skipped" 'not-found
  (cond ((assv 'z '((a 1) (b 2))) => cadr) (else 'not-found)))
(test-equal "=> accepts a lambda" '(found (b c))
  (cond ((memq 'b '(a b c)) => (lambda (x) (list 'found x))) (else #f)))

;; ── 4.2.1 case ─────────────────────────────────────────────────────────────

(test-equal "case matches a datum in a list" 'composite
  (case (* 2 3) ((2 3 5 7) 'prime) ((1 4 6 8 9) 'composite)))
(test-equal "case falls through to else" 'consonant
  (case (car '(c d)) ((a e i o u) 'vowel) ((w y) 'semivowel) (else 'consonant)))
(test-equal "case's else => passes the key" 'c
  (case (car '(c d))
    ((a e i o u) 'vowel)
    ((w y) 'semivowel)
    (else => (lambda (x) x))))
(test-equal "a single-datum clause" 'matched (case 'a ((a) 'matched)))
(test-equal "a clause runs its expressions in order" 15
  (let ((x 0))
    (case 1 ((1) (set! x 10) (set! x (+ x 5)) x))))

;; A clause with datums but no body. R7RS's grammar wants at least one
;; expression, but chibi and Gauche both accept the empty body with an
;; unspecified result, and real packages ship it — chibi-tar's `((#\g #\x))`
;; metadata clause is how this surfaced (it classified the whole package as
;; parse-error). Empty `(else)` is the same decision.
(test-equal "an unmatched empty clause lets dispatch continue" 'other
  (case 2 ((1)) (else 'other)))
;; The portable half of the next row: a matched empty clause ends the dispatch
;; there, so the else clause never runs.
(test-assert "a matched empty clause stops the dispatch"
  (let ((else-ran #f))
    (case 1 ((1)) (else (set! else-ran #t)))
    (not else-ran)))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an empty clause, a lone empty clause and an empty else are unspecified"
  '("#<unspecified>" "#<unspecified>" "#<unspecified>")
  (list (written (case 1 ((1)) (else 'other)))
        (written (case 1 ((1))))
        (written (case 2 ((1) 'one) (else)))))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a case that matches nothing is unspecified" "#<unspecified>"
  (written (case 10 ((1 2 3) 'small) ((20 30) 'big))))

;; ── 4.2.1 and, or ──────────────────────────────────────────────────────────

(test-equal "and of true tests" #t (and (= 2 2) (> 2 1)))
(test-equal "and with a false test" #f (and (= 2 2) (< 2 1)))
(test-equal "and returns its last value" '(f g) (and 1 2 'c '(f g)))
(test-equal "and of nothing" #t (and))
(test-equal "or of true tests" #t (or (= 2 2) (> 2 1)))
(test-equal "or of false tests" #f (or #f #f #f))
;; The `(/ 3 0)` is never evaluated: `or` stops at the first true value.
(test-equal "or returns the first true value" '(b c)
  (or (memq 'b '(a b c)) (/ 3 0)))

;; ── 4.2.1 when, unless ─────────────────────────────────────────────────────

(test-equal "when with a true test" 'yes (when (> 3 2) 'yes))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "when with a false test is unspecified" "#<unspecified>"
  (written (when (< 3 2) 'yes)))
(test-equal "unless with a false test" 'yes (unless (< 3 2) 'yes))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "unless with a true test is unspecified" "#<unspecified>"
  (written (unless (> 3 2) 'yes)))

;; ── 4.2.2 let, let*, letrec ────────────────────────────────────────────────

(test-equal "let" 6 (let ((x 2) (y 3)) (* x y)))
;; The inner `x` shadows the outer, but `z`'s init sees the outer one.
(test-equal "let's inits see the enclosing scope" 35
  (let ((x 2) (y 3)) (let ((x 7) (z (+ x y))) (* z x))))
;; `let*` is sequential, so `z`'s init sees the new `x`.
(test-equal "let* binds in order" 70
  (let ((x 2) (y 3)) (let* ((x 7) (z (+ x y))) (* z x))))
(test-equal "letrec allows mutual recursion" #t
  (letrec ((even? (lambda (n) (if (= n 0) #t (odd? (- n 1)))))
           (odd? (lambda (n) (if (= n 0) #f (even? (- n 1))))))
    (even? 88)))

;; ── 4.2.2 let-values, let*-values ──────────────────────────────────────────

(test-equal "let-values" 3 (let-values (((a b) (values 1 2))) (+ a b)))
(test-equal "let-values with two bindings" 10
  (let-values (((a b) (values 1 2)) ((c d) (values 3 4))) (+ a b c d)))
;; From chibi's tests: the second binding's `x` and `y` are new, and see the
;; first binding's `a` and `b`.
(test-equal "let*-values binds in order" '(x y x y)
  (let ((x 'x) (y 'y))
    (let*-values (((a b) (values x y))
                  ((x y) (values a b)))
      (list a b x y))))
(test-equal "let*-values inits see earlier bindings" '(1 2 1 2)
  (let*-values (((a b) (values 1 2)) ((c d) (values a b))) (list a b c d)))
(test-equal "let-values with no bindings" 42 (let-values () 42))

;; ── 4.2.4 do ───────────────────────────────────────────────────────────────

(test-equal "do with two stepped variables" 15
  (do ((i 0 (+ i 1))
       (sum 0 (+ sum i)))
      ((> i 5) sum)))
(test-equal "do runs its commands each iteration" '(2 1 0)
  (let ((result '()))
    (do ((i 0 (+ i 1)))
        ((= i 3) result)
      (set! result (cons i result)))))
(test-equal "a variable without a step keeps its value" 'done
  (do ((x 5)) ((= x 5) 'done)))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "do with no result expressions is unspecified" "#<unspecified>"
  (written (do ((i 0 (+ i 1))) ((> i 5)))))
(test-equal "do returns its last result expression" 'last
  (do ((i 0 (+ i 1)))
      ((> i 3) 'first 'second 'last)))
(test-equal "the report's vector example" #(0 1 2 3 4)
  (do ((vec (make-vector 5))
       (i 0 (+ i 1)))
      ((= i 5) vec)
    (vector-set! vec i i)))
(test-equal "the report's list-sum example" 25
  (let ((x '(1 3 5 7 9)))
    (do ((x x (cdr x))
         (sum 0 (+ sum (car x))))
        ((null? x) sum))))
(test-equal "factorial with do" 120
  (do ((n 5 (- n 1))
       (result 1 (* result n)))
      ((= n 0) result)))
;; The test is true on entry, so the commands never run.
(test-equal "a do whose test starts true runs no commands" 0
  (let ((counter 0))
    (do ((i 10))
        (#t counter)
      (set! counter (+ counter 1)))))
(test-equal "stepped and unstepped variables together" 5
  (do ((i 0 (+ i 1))
       (limit 5))
      ((= i limit) i)))

;; ── 4.3 syntax-rules over the derived forms ────────────────────────────────

(define-syntax test-when
  (syntax-rules ()
    ((test-when test body ...)
     (if test (begin body ...)))))

(test-equal "a when-like macro with a true test" 42 (test-when #t 42))
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a when-like macro with a false test is unspecified" "#<unspecified>"
  (written (test-when #f 42)))

(define-syntax my-when
  (syntax-rules ()
    ((my-when test body ...)
     (if test (begin body ...)))))

(define-syntax my-unless
  (syntax-rules ()
    ((my-unless test body ...)
     (if (not test) (begin body ...)))))

;; The inner macro use is not renamed away by the outer expansion.
(test-equal "a macro use inside another's argument" 42
  (my-when #t
    (my-unless #f
      42)))
(test-equal "macro uses nested four deep" 123
  (my-when #t
    (my-unless #f
      (my-when #t
        (my-unless #f
          123)))))

;; Hygiene: the macro's own `temp` is renamed, so the program's `temp` is
;; neither read nor overwritten by the expansion.
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((temp a))
       (set! a b)
       (set! b temp)))))

(define temp 999)
(define x 1)
(define y 2)
(swap! x y)
(test-equal "a macro's temporary does not capture the program's" '(2 1 999)
  (list x y temp))

(test-end)
