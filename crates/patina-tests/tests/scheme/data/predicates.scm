;; Booleans, equivalence and the type predicates — R7RS §6.1 and §6.3, with
;; `procedure?` from §6.10.
;;
;; Migrated from the non-numeric half of
;; `crates/patina-tests/tests/compliance/predicates.rs` (#193), which is
;; deleted; the numeric predicates went to `numeric-predicates.scm`. One row
;; here per Rust assertion, less exact duplicates: `(string? "hello")`,
;; `(string? 'hello)` and `(string? 42)` are rows of `strings.scm`, and
;; `(vector? #(1 2 3))` one of `vectors.scm`, with the same answers, so they
;; are not repeated here. `(vector? '(1 2 3))` is not a duplicate and stays.
;;
;; Programs that defined a procedure and then asked about it are `let ()`
;; bodies with internal definitions.

(import (scheme base) (scheme eval) (srfi 64))

(test-begin "predicates")

;; ─── Booleans ────────────────────────────────────────────────────────────────

(test-equal "#t evaluates to itself" #t #t)
(test-equal "#f evaluates to itself" #f #f)

(test-equal "boolean? of #t" #t (boolean? #t))
(test-equal "boolean? of #f" #t (boolean? #f))
(test-equal "boolean? of 0" #f (boolean? 0))
(test-equal "boolean? of the empty list" #f (boolean? '()))

;; Everything except #f counts as true.
(test-equal "not #t" #f (not #t))
(test-equal "not #f" #t (not #f))
(test-equal "not 0" #f (not 0))
(test-equal "not of the empty list" #f (not '()))

(test-equal "boolean=? of two #t" #t (boolean=? #t #t))
(test-equal "boolean=? of two #f" #t (boolean=? #f #f))
(test-equal "boolean=? of #t and #f" #f (boolean=? #t #f))
(test-equal "boolean=? of three #t" #t (boolean=? #t #t #t))
(test-equal "boolean=? of three #f" #t (boolean=? #f #f #f))
(test-equal "boolean=? of three, one different" #f (boolean=? #t #t #f))

;; ─── Equivalence ─────────────────────────────────────────────────────────────

(test-equal "eq? of the same symbol" #t (eq? 'a 'a))
(test-equal "eq? of different symbols" #f (eq? 'a 'b))
(test-equal "eq? of #t and #t" #t (eq? #t #t))
(test-equal "eq? of #f and #f" #t (eq? #f #f))

(test-equal "eqv? of the same symbol" #t (eqv? 'a 'a))
(test-equal "eqv? of equal small integers" #t (eqv? 2 2))
(test-equal "eqv? of different integers" #f (eqv? 2 3))
(test-equal "eqv? of empty lists" #t (eqv? '() '()))

(test-equal "equal? of the same symbol" #t (equal? 'a 'a))
(test-equal "equal? of equal lists" #t (equal? '(a b) '(a b)))
(test-equal "equal? of different lists" #f (equal? '(a b) '(a c)))

;; ─── Type predicates ─────────────────────────────────────────────────────────

(test-equal "symbol? of a symbol" #t (symbol? 'a))
(test-equal "symbol? of a number" #f (symbol? 42))
(test-equal "symbol? of a string" #f (symbol? "a"))

(test-equal "char? of a character" #t (char? #\a))
(test-equal "char? of a string" #f (char? "a"))
(test-equal "char? of a number" #f (char? 97))
(test-equal "char? of a CJK character" #t (char? #\世))

(test-equal "vector? of a list" #f (vector? '(1 2 3)))

;; ─── procedure? ──────────────────────────────────────────────────────────────

(test-equal "procedure? of +" #t (procedure? +))
(test-equal "procedure? of cons" #t (procedure? cons))
(test-equal "procedure? of a lambda" #t (procedure? (lambda (x) x)))
(test-equal "procedure? of a thunk" #t (procedure? (lambda () 3)))
;; R7RS §4.2.6: make-parameter returns a procedure. What follows from that is
;; `control/parameters.scm`'s subject.
(test-equal "procedure? of a parameter object" #t (procedure? (make-parameter 1)))

(test-equal "procedure? of a number" #f (procedure? 42))
(test-equal "procedure? of #t" #f (procedure? #t))
(test-equal "procedure? of a string" #f (procedure? "hello"))
(test-equal "procedure? of a symbol" #f (procedure? 'symbol))
(test-equal "procedure? of the empty list" #f (procedure? '()))
(test-equal "procedure? of a list" #f (procedure? '(1 2 3)))
(test-equal "procedure? of a pair" #f (procedure? (cons 1 2)))
(test-equal "procedure? of a vector" #f (procedure? (vector 1)))

(test-equal "procedure? of a call's value" #f (procedure? ((lambda () 3))))
(test-equal "procedure? of a call returning +" #t (procedure? ((lambda () +))))
(test-equal "procedure? of itself" #t (procedure? procedure?))
(test-equal "procedure? of a lambda returning +" #t (procedure? (lambda () +)))

(test-equal "procedure? of a defined procedure" #t
  (let ()
    (define (make-adder n) (lambda (x) (+ x n)))
    (procedure? make-adder)))
(test-equal "procedure? of a closure it returns" #t
  (let ()
    (define (make-adder n) (lambda (x) (+ x n)))
    (procedure? (make-adder 5))))
(test-equal "procedure? of a recursive procedure" #t
  (let ()
    (define (factorial n) (if (= n 0) 1 (* n (factorial (- n 1)))))
    (procedure? factorial)))
(test-equal "procedure? of a curried procedure" #t
  (let ()
    (define (curry f) (lambda (x) (lambda (y) (f x y))))
    (procedure? (curry +))))
(test-equal "procedure? of a curried procedure applied once" #t
  (let ()
    (define (curry f) (lambda (x) (lambda (y) (f x y))))
    (procedure? ((curry +) 5))))

;; (apply + '()) is 0; (apply (lambda () +) '()) is +.
(test-equal "procedure? of apply's number" #f (procedure? (apply + '())))
(test-equal "procedure? of apply's procedure" #t (procedure? (apply (lambda () +) '())))
(test-equal "procedure? of map's list" #f (procedure? (map (lambda (x) x) '(1 2))))
(test-equal "procedure? of a procedure map returned" #t
  (procedure? (car (map (lambda (x) +) '(1)))))

;; Syntax is not a value at all, so `(procedure? if)` is refused rather than
;; answered — core keywords and the derived forms `(scheme base)` defines as
;; macros alike. `syntax_as_a_value.rs` owns that rule and its reasoning; this
;; row only keeps the predicate from drifting back to answering. The row
;; returns the keywords that were answered rather than refused, so a failure
;; names them. Each is asked through `eval`, because a program that used one
;; as a value directly would be refused before any row ran.
;;
;; Scoped to Patina, because R7RS makes a keyword used as a value an error and
;; neither oracle can arbitrate it. Gauche 0.9.15 answers #f for all twelve.
;; chibi 0.12 refuses all twelve, but measured 2026-09-11 its `eval` then
;; re-enters the loop for the eight derived forms after the program has ended,
;; printing once per combination of them, which would corrupt the file's
;; counts.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "no keyword is answered as a procedure" '()
  (let ((env (environment '(scheme base))))
    (let loop ((ks '(if lambda define quote
                     let let* letrec letrec* and or cond case))
               (answered '()))
      (if (null? ks)
          (reverse answered)
          (let ((refused (guard (e (#t #t))
                           (eval (list 'procedure? (car ks)) env)
                           #f)))
            (loop (cdr ks) (if refused answered (cons (car ks) answered))))))))

(test-end)
