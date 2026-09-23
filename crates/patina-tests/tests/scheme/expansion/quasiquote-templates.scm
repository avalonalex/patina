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

;; ── An unquoted expression is code in the form around it ───────────────────
;;
;; #445. The expression inside an `unquote` is ordinary code where the template
;; stands, so the local keywords and variables of that form are its own. It
;; used to be desugared afterwards, against the global environment alone: a
;; keyword bound by `let-syntax` or an internal `define-syntax` was unbound
;; inside an unquote, and a local variable spelled like a global macro — or
;; like `apply` — was taken for it, silently wherever the macro accepted the
;; call. chibi 0.12 and Gauche 0.9.15 answer every row as written, measured
;; 2026-09-22.
(test-equal "a let-syntax keyword inside an unquote" '(a expanded)
  (let-syntax ((m (syntax-rules () ((_) 'expanded)))) `(a ,(m))))
(test-equal "an internal define-syntax keyword inside an unquote" '(a internal)
  (let () (define-syntax m (syntax-rules () ((_) 'internal))) `(a ,(m))))
(test-equal "a local keyword in a vector template and a splice" #(v 2 3)
  (let-syntax ((m (syntax-rules () ((_ x) (+ x 1))))) `#(v ,(m 1) ,@(list (m 2)))))
(test-equal "a local keyword in an unquoted dotted tail" '(a . tail)
  (let-syntax ((m (syntax-rules () ((_) 'tail)))) `(a . ,(m))))
(test-equal "a local keyword at depth zero inside a nested template"
  '(a `(b ,(c deep)))
  (let-syntax ((m (syntax-rules () ((_) 'deep)))) `(a `(b ,(c ,(m))))))

(define (call-when when) `(a ,(when 1)))
(test-equal "a parameter spelled like a macro is called, not expanded" '(a (called 1))
  (call-when (lambda (x) (list 'called x))))
(test-equal "a variable spelled like a macro is a value" '(a 3)
  (let ((unless 3)) `(a ,unless)))
(test-equal "a local apply is the local procedure" '(local)
  (let ((apply (lambda (f xs) 'local))) `(,(apply + '(1 2)))))

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

;; An `unquote` with no operand at all — the zero end of the same `*`.
;;
;; R6RS 11.17 writes `(unquote <qq template D-1>*)`, so none is as legal as
;; three and inserts nothing. These rows asserted a *refusal* until multi-
;; operand unquote was adopted, because the decision had not been taken and
;; refusing was what not reading past the end of the form left behind: the
;; VM's expansion used to read the operand with an unchecked `car`, which
;; panicked a debug build and in release read whatever the tagged value
;; pointed at, answering ``(a `(b ,syntax))`` for the third row below and
;; naming a symbol that appears nowhere in the program. That read is still
;; checked; what changed is that a well-formed empty operand list is now an
;; answer rather than an error.
;;
;; Gauche agrees on all three. chibi refuses them, as Patina used to, and the
;; register carries that.
(test-equal "an unquote with no operand inserts nothing" '(a)
  (eval '`(a (unquote)) (environment '(scheme base))))
(test-equal "an unquote-splicing with no operand splices nothing" '(a)
  (eval '`(a (unquote-splicing)) (environment '(scheme base))))
(test-equal "a nested template rebuilds an unquote with no operand"
  '(a (quasiquote (b (unquote))))
  (eval '`(a `(b (unquote))) (environment '(scheme base))))

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

;; ── A splice in the last position is append's last argument ────────────────

;; R7RS §4.2.8 makes a non-list splice an error wherever it stands, and an
;; error it need not signal, so what the last position answers is Patina's
;; choice. It is the VM's: templates compile to `append`, so `(a ,@x)` is
;; `(append (list 'a) x)`, and append's last argument "can be of any type"
;; (§6.4). A non-list therefore makes an improper list, and a list is the tail
;; itself rather than a copy. chibi 0.12 and Gauche 0.9.15 make the same
;; choice. The tree-walker, which evaluates templates directly, refused a
;; non-list anywhere and copied a list, until #270 made it follow the VM.
;;
;; A vector template is `(list->vector (append ...))`, so its last splice is
;; not a tail, and must be a list like any other splice.
;;
;; chibi agrees with every row below except two of the three errors, where it
;; keeps the latitude registered for the row above; it signals on the vector
;; row. Values arrive as procedure arguments, for the reason given above.
(define (splice-last n) `(a ,@n))
(define (splice-alone n) `(,@n))
(define (splice-twice m n) `(a ,@m ,@n))
(define (splice-before-dotted-tail n) `(a ,@n . b))
(define (splice-into-vector n) `#(a ,@n))
(define (splice-nested n) `(a `(b ,(c ,@n))))

(test-equal "a non-list spliced last becomes the tail" '(a . 42)
  (splice-last 42))
(test-equal "a non-list spliced alone is the whole value" 42
  (splice-alone 42))
(test-equal "an improper list spliced last keeps its tail" '(a 1 . 2)
  (splice-last '(1 . 2)))
(test-equal "only the last of two splices may be a non-list" '(a 1 . 42)
  (splice-twice '(1) 42))
;; R7RS lets a quasiquote share structure it need not rebuild, so copying
;; would not be wrong. The row pins the two backends to one answer, which is
;; also chibi's and Gauche's.
(test-assert "a list spliced last is shared, not copied"
  (let ((n (list 1 2)))
    (eq? (cdr (splice-last n)) n)))
;; The inner template's `,@n` is at nesting level zero, so it is evaluated,
;; and the same rule applies there.
(test-equal "a splice evaluated inside a nested template follows the rule"
  '(a `(b ,(c . 42)))
  (splice-nested 42))

;; Last means nothing follows, not only splices. An implementation that took
;; "the cdr is not a pair" for "last" would accept the first of these as a
;; tail and drop the `b`.
(test-error "a splice before a dotted tail must be a list" #t
  (splice-before-dotted-tail 42))
(test-error "the first of two splices must be a list" #t
  (splice-twice 42 '(1)))
;; A vector cannot have a tail, so list->vector refuses the improper list.
(test-error "a vector template cannot end in a non-list splice" #t
  (splice-into-vector 42))

;; #275: circular splices must not make append/list->vector allocate forever.
;; Chibi 0.12 and Gauche 0.9.15 timed out on middle/dotted-tail splices in
;; bounded subprocess probes (2026-09-12), but both catch the vector case.
(define circular-splice (list 1 2 3))
(set-cdr! (cddr circular-splice) (cdr circular-splice))
(cond-expand ((or chibi gauche) (test-skip 1)) (else))
(test-error "a circular middle splice is rejected" #t
  (splice-into-middle circular-splice))
(cond-expand ((or chibi gauche) (test-skip 1)) (else))
(test-error "a circular splice before a dotted tail is rejected" #t
  (splice-before-dotted-tail circular-splice))
(test-error "a circular final vector splice is rejected" #t
  (splice-into-vector circular-splice))
;; Preserve the established final-tail sharing rule for list templates.
(test-assert "a circular final list splice remains shared"
  (eq? (cdr (splice-last circular-splice)) circular-splice))


;; ── Shapes R7RS does not describe, and the two backends now agree on ────────
;;
;; Issue #276. The VM compiled templates and the tree-walker evaluated them
;; with a separate walker, and the two derived "last", "list context" and
;; "tail" independently — so on every shape below the VM answered and the
;; tree-walker refused. One derivation of the template, made by the desugarer
;; before either backend sees it, is what makes them agree; these rows are
;; what says so.
;;
;; R7RS §7.1.4 gives `unquote` one template and §4.2.8 makes anything else an
;; error, so none of these has a *required* answer and the oracles split. Two
;; of them do have a chosen one: Patina reads R6RS 11.17's
;; `(unquote <qq template>*)`, so a multi-operand unquote inserts every
;; operand and the splicing spelling splices every one. That is an extension,
;; taken deliberately — Gauche, Chez and Larceny read it the same way, and
;; Larceny's suite asserts it — and chibi and Racket take the other reading,
;; which the register carries. What is *not* latitude is Patina giving two
;; answers to one program, which is what all of these pin.
;;
;; Written through `eval` for the reason the rows above give: Patina decides
;; these while desugaring the form, so a row holding one directly would settle
;; the whole file's fate rather than its own.

(test-equal "a splice outside a list template" '(1 2)
  (eval '(let ((x '(1 2))) `,@x) (environment '(scheme base))))

(test-equal "an unquote in a dotted tail with extra operands" '(a unquote x y)
  (eval '(let ((x '(1 2)) (y 9)) `(a unquote x y)) (environment '(scheme base))))

(test-equal "unquote-splicing as a vector template's head" '#(unquote-splicing x)
  (eval '(let ((x '(1 2))) `#(unquote-splicing x)) (environment '(scheme base))))

(test-equal "a multi-operand unquote in element position" '(foo foo foo)
  (eval '(let ((x 'foo)) `((unquote x x x))) (environment '(scheme base))))

(test-equal "a multi-operand unquote-splicing in element position" '(a a)
  (eval '(let ((x '(a))) `((unquote-splicing x x))) (environment '(scheme base))))

;; A vector template goes through the same element walk, lowered to
;; `list->vector` over the same segments, so the extension reaches it too.
;; Pinned separately because "the same walk" is an implementation fact and
;; this is the observable one.
(test-equal "a multi-operand unquote in a vector template" '#(a p p b)
  (eval '(let ((x 'p)) `#(a (unquote x x) b)) (environment '(scheme base))))

(test-equal "a multi-operand unquote-splicing in a vector template" '#(a 1 2 1 2 b)
  (eval '(let ((x '(1 2))) `#(a (unquote-splicing x x) b)) (environment '(scheme base))))

;; A template that *is* an unquote has no list to splice into, so several
;; operands are refused rather than truncated to the first — which is what
;; this did before the extension existed, and would now be the one answer
;; that quietly discards what the program wrote.
(test-error "a template that is itself a multi-operand unquote is refused" #t
  (eval '(let ((a 1) (b 2)) `(unquote a b)) (environment '(scheme base))))

;; An operand list must be proper and finite at every depth. The reader
;; accepts a datum label, so a template can hold a circular one; walking it
;; without a cycle check allocated until the process died.
;;
;; chibi refuses all three, for reasons of its own — "invalid use of auxiliary
;; syntax" for the circular list, "car: not a pair" for both improper ones —
;; which is as much as a `test-error` row asks. Gauche cannot run the first
;; two at all: compiling the circular template allocates without bound, and
;; the flat improper one segfaults gosh, both measured 2026-09-13 inside a
;; procedure that is never called. So those two are skipped on Gauche alone,
;; one skip each, so that neither can drift onto a neighbouring row when rows
;; move; `test-skip` prevents evaluation, which keeps two rows from costing the
;; file its whole Gauche column. Gauche does run the nested one, and rebuilds
;; the template where Patina refuses it; that difference is registered.
;;
;; All three were once skipped on every oracle, because Gauche "completed the
;; circular row on macOS and did not complete this file on CI". It never
;; completed that row. The lane's alarm was delivered to Gauche, which raised
;; it as a catchable error, so `test-error` scored a spin of the whole timeout
;; as a pass; on CI the memory limit aborted the file first. The lane now
;; kills a timed-out oracle and caps Gauche's heap, and its self-checks prove
;; both before it trusts a result, so a spin like that fails the file on every
;; platform instead of passing a row.
(cond-expand (gauche (test-skip 1)) (else))
(test-error "a circular operand list is refused" #t
  (eval '`(a #0=(unquote . #0#)) (environment '(scheme base))))
(cond-expand (gauche (test-skip 1)) (else))
(test-error "an improper operand list is refused" #t
  (eval '(let ((x 1)) `(a (unquote . x))) (environment '(scheme base))))
(test-error "and is refused inside a nested template too" #t
  (eval '(let ((x 1)) ``(a (unquote . x))) (environment '(scheme base))))

(test-end)
