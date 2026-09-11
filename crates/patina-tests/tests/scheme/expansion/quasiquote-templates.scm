;; Quasiquote templates — R7RS §4.2.8: `unquote`, `unquote-splicing`, vector
;; templates, nesting levels, and the report's own examples.
;;
;; Migrated from `crates/patina-tests/tests/compliance/quasiquote.rs` (#193),
;; which is deleted. A sibling of `quasiquote.scm` rather than a section of it,
;; because that file's import set is its test: it replaces `(scheme base)`'s
;; `quote`, `list` and `append` with SRFI 101's, so an ordinary `'(a b)` there
;; is a random-access list and none of these rows could be written in it.
;;
;; The Rust file compared printed forms, and wrote nested templates back with
;; their abbreviations — `(a `(b ,x) e)`. Here the expected values are quoted
;; data compared with `equal?`, and the reader expands those abbreviations into
;; the same `quasiquote` and `unquote` lists the template evaluates to, so the
;; rows say what structure a template builds. Whether a writer abbreviates is
;; printing latitude, and chibi declines to.
;;
;; Four of the Rust file's "R7RS specification examples" repeated an earlier
;; test character for character; each appears once here, as the example it is.

(import (scheme base) (scheme eval) (srfi 64))

(test-begin "quasiquote-templates")

;; ── Without unquote, quasiquote is quote ───────────────────────────────────

(test-equal "a list of symbols" '(a b c) `(a b c))
(test-equal "a list of numbers" '(1 2 3) `(1 2 3))
(test-equal "a number" 42 `42)
(test-equal "a boolean" #t `#t)
(test-equal "a string" "hello" `"hello")
(test-equal "a symbol" 'foo `foo)
(test-equal "a list of two symbols" '(foo bar) `(foo bar))
(test-equal "the empty list" '() `())
(test-equal "a pair" '(a . b) `(a . b))
(test-equal "an improper list" '(a b . c) `(a b . c))
(test-equal "a vector" #(1 2 3) `#(1 2 3))

;; ── unquote ────────────────────────────────────────────────────────────────

(test-equal "unquote, the report's first example" '(list 3 4) `(list ,(+ 1 2) 4))
(test-equal "several unquotes" '(3 12 5) `(,(+ 1 2) ,(* 3 4) 5))
(test-equal "an unquoted if" '(a yes b) `(a ,(if #t 'yes 'no) b))
(test-equal "unquoted car and cdr" '(1 (2 3)) `(,(car '(1 2 3)) ,(cdr '(1 2 3))))
(test-equal "an unquoted list is one element" '(a (1 2 3) b) `(a ,(list 1 2 3) b))

(define name 'a)
(test-equal "an unquote inside a quote inside the template" '(list a (quote a))
  `(list ,name ',name))
(test-equal "the same with name bound by let, the report's second example"
  '(list a (quote a))
  (let ((name 'a))
    `(list ,name ',name)))

(test-equal "the long forms" '(list 3 4) (quasiquote (list (unquote (+ 1 2)) 4)))
;; The reader's abbreviations and the long forms are the same datum.
(test-equal "a quoted quasiquote is its abbreviation" '`(list ,(+ 1 2) 4)
  '(quasiquote (list (unquote (+ 1 2)) 4)))

;; ── unquote-splicing ───────────────────────────────────────────────────────

(test-equal "splicing, the report's third example" '(a 3 4 5 6 b)
  `(a ,(+ 1 2) ,@(map abs '(4 -5 6)) b))
(test-equal "splicing at the start" '(1 2 3 4 5) `(,@(list 1 2 3) 4 5))
(test-equal "splicing at the end" '(1 2 3 4 5) `(1 2 ,@(list 3 4 5)))
(test-equal "splicing the empty list leaves nothing" '(a b) `(a ,@'() b))
(test-equal "two splices" '(1 2 3 4) `(,@(list 1 2) ,@(list 3 4)))
;; The report's fourth example: a splice of nothing followed by a dotted tail.
(test-equal "a splice before an unquoted dotted tail" '((foo 7) . cons)
  `((foo ,(- 10 3)) ,@(cdr '(c)) . ,(car '(cons))))
;; The report writes `(sqrt 4)` and `(map sqrt '(16 9))`; the Rust file used
;; `square` so every element stays exact.
(test-equal "a vector template with unquote and splicing" #(10 5 4 16 9 8)
  `#(10 5 ,(square 2) ,@(map square '(4 3)) 8))

;; ── Nesting ────────────────────────────────────────────────────────────────

;; Only the innermost `,(+ 1 3)` is at depth zero, so only it is evaluated.
(test-equal "a nested quasiquote, the report's fifth example"
  '(a `(b ,(+ 1 2) ,(foo 4 d) e) f)
  `(a `(b ,(+ 1 2) ,(foo ,(+ 1 3) d) e) f))

(define name1 'x)
(define name2 'y)
(test-equal "unquote twice and quote-unquote at depth two"
  '(a `(b ,x ,'y d) e)
  `(a `(b ,,name1 ,',name2 d) e))
(test-equal "the same with let, the report's sixth example"
  '(a `(b ,x ,'y d) e)
  (let ((name1 'x) (name2 'y))
    `(a `(b ,,name1 ,',name2 d) e)))

(test-equal "a double unquote reaches depth zero" '`(a ,10)
  (let ((x 10))
    ``(a ,,x)))
(test-equal "three levels deep" '``(a ,,1)
  (let ((x 1))
    ```(a ,,,x)))

;; ── In use ─────────────────────────────────────────────────────────────────

(test-equal "a template over let-bound variables" '(sum is 30)
  (let ((x 10) (y 20))
    `(sum is ,(+ x y))))

(define (make-adder n)
  `(lambda (x) (+ x ,n)))
(test-equal "a template that builds code" '(lambda (x) (+ x 5)) (make-adder 5))

(define nums '(1 2 3))
(test-equal "splicing a map" '(results: 2 4 6)
  `(results: ,@(map (lambda (x) (* x 2)) nums)))

;; ── Errors ─────────────────────────────────────────────────────────────────

;; `unquote` and `unquote-splicing` outside a template are refused while the
;; form is compiled, on Patina, chibi and Gauche alike, so a row cannot hold
;; one directly — the file would not compile. `eval` moves the refusal to run
;; time, where `test-error` sees it. The Rust rows wrote `(unquote x)` with `x`
;; unbound, which an implementation treating `unquote` as an ordinary
;; procedure would also fail — on the unbound `x`. The operands here evaluate
;; fine, so only refusing the keyword makes these rows pass.
(test-error "unquote outside a quasiquote is an error" #t
  (eval '(unquote 1) (environment '(scheme base))))
(test-error "unquote-splicing outside a quasiquote is an error" #t
  (eval '(unquote-splicing '(1)) (environment '(scheme base))))

;; R7RS §4.2.8 says a spliced expression "must evaluate to a list", which is
;; an "is an error" case: an implementation may signal it or not. Patina and
;; Gauche signal; chibi splices the 42 as if it were empty and answers (a b),
;; which the register records as latitude.
;;
;; The first row splices a procedure's argument, so the value arrives at run
;; time. A `let`-bound 42 would say the same thing on Patina, but Gauche 0.9.15
;; folds that constant into the template and its `test-error` then reports no
;; error at all, though a `guard` around the same expression catches one. The
;; second row splices the literal the Rust file wrote, which Gauche refuses
;; while compiling ("proper list required, but got 42"), taking the file with
;; it, so it sits in a clause Gauche does not select.
(define (splice-into-middle n) `(a ,@n b))
(test-error "splicing a non-list value is an error" #t (splice-into-middle 42))
(cond-expand
  (gauche)
  (else
   (test-error "splicing a non-list literal is an error" #t `(a ,@42 b))))

(test-end)
