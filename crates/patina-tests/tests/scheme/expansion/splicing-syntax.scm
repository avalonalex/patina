;; SRFI 188 and #424: splice definitions, retain local keyword scope.
;; Original tests, checked with Chibi's SRFI 188 and Chez's R6RS forms.
;; Gauche 0.9.15 has no (srfi 188); its missing capability is registered.
(import (scheme base) (scheme eval) (srfi 64) (srfi 188)
        (prefix (srfi 188) renamed:))
(test-begin "splicing-syntax")

;; Chibi 0.12 rejects zero-form bodies at expansion time. SRFI 188's
;; <form> ... grammar and Chez's R6RS forms allow them (#424).
(cond-expand
 (chibi)
 (else
(test-equal "an empty splice contributes no forms" 5
  (let () (splicing-let-syntax ()) 5))
(test-equal "an empty recursive splice contributes no forms" 6
  (let () (splicing-letrec-syntax ()) 6))))
(test-equal "a direct definition belongs to the surrounding body" 42
  (let () (splicing-let-syntax () (define answer 42)) answer))
(test-equal "a macro-generated definition belongs to the surrounding body" 43
  (let ()
    (splicing-let-syntax ((def (syntax-rules () ((_ name value) (define name value)))))
      (def answer 43))
    answer))
(test-equal "a begin inside a splice contributes its definitions" '(1 2)
  (let () (splicing-let-syntax () (begin (define a 1) (define b 2))) (list a b)))
(test-equal "nested splices contribute to the same body" '(1 2)
  (let ()
    (splicing-let-syntax ()
      (define a 1)
      (splicing-letrec-syntax () (define b 2)))
    (list a b)))
(test-equal "definitions and expressions may share a splice" '(inner inner)
  (let ((x 'outer))
    (let ()
      (define observed #f)
      (splicing-let-syntax () (define x 'inner) (set! observed x))
      (list x observed))))
(test-equal "ordinary let-syntax still has a separate body" 'outer
  (let ((x 'outer)) (let-syntax () (define x 'inner) x) x))
(test-equal "ordinary let-syntax inside a splice keeps its definitions" 'outer
  (let ((x 'outer))
    (splicing-let-syntax () (let-syntax () (define x 'inner) x))
    x))
(test-equal "a lambda inside a splice keeps its definitions" 'outer
  (let ((x 'outer))
    (splicing-let-syntax () ((lambda () (define x 'inner) x)))
    x))

(define-syntax outside (syntax-rules () ((_) 'outer)))
(test-equal "temporary keywords are visible only inside the splice" '(inner outer)
  (let ()
    (define seen #f)
    (splicing-let-syntax ((outside (syntax-rules () ((_) 'inner))))
      (set! seen (outside)))
    (list seen (outside))))
(test-equal "nonrecursive transformers use the enclosing keyword" 'outer
  (splicing-let-syntax ((outside (syntax-rules () ((_) 'inner)))
                       (call-it (syntax-rules () ((_) (outside)))))
    (call-it)))
(test-equal "recursive transformers can use a sibling keyword" 'inner
  (splicing-letrec-syntax ((outside (syntax-rules () ((_) 'inner)))
                          (call-it (syntax-rules () ((_) (outside)))))
    (call-it)))
(test-equal "a recursive transformer can use itself" '(a b c)
  (splicing-letrec-syntax
      ((collect (syntax-rules () ((_ ) '()) ((_ x xs ...) (cons 'x (collect xs ...))))))
    (collect a b c)))
(test-equal "a syntax definition also belongs to the surrounding body" 44
  (let ()
    (splicing-let-syntax () (define-syntax answer (syntax-rules () ((_) 44))))
    (answer)))
(test-equal "an exported macro retains its local helper keyword" 45
  (let ()
    (splicing-let-syntax ((helper (syntax-rules () ((_) 45))))
      (define-syntax answer (syntax-rules () ((_) (helper)))))
    (answer)))
(test-equal "a generated splice preserves caller-supplied names" 46
  (let ()
    (define-syntax make-definition
      (syntax-rules ()
        ((_ name)
         (splicing-let-syntax ((def (syntax-rules () ((_ n) (define n 46)))))
           (def name)))))
    (make-definition answer)
    answer))
(test-equal "a generated splice does not capture an enclosing variable" 'outer
  (let ((hidden 'outer))
    (define-syntax introduce
      (syntax-rules () ((_ ) (splicing-let-syntax () (define hidden 'inner)))))
    (introduce)
    hidden))
(test-equal "spliced definitions shadow syntax throughout the body" '(47 47)
  (let ()
    (define (earlier) if)
    (splicing-let-syntax () (define if 47))
    (list (earlier) if)))
(test-equal "a spliced procedure definition keeps recursive scope" 120
  (let ()
    (splicing-let-syntax ()
      (define (factorial n) (if (= n 0) 1 (* n (factorial (- n 1))))))
    (factorial 5)))
(test-equal "renaming preserves the splicing binding" 48
  (let () (renamed:splicing-let-syntax () (define answer 48)) answer))
(test-equal "renaming preserves the recursive splicing binding" 49
  (let () (renamed:splicing-letrec-syntax () (define answer 49)) answer))
(test-equal "an expression splice preserves multiple values" '(1 2)
  (call-with-values (lambda () (splicing-let-syntax () (values 1 2))) list))
(test-equal "a splice does not add a runtime frame to tail recursion" 2000
  (let loop ((n 2000) (count 0))
    (if (= n 0) count
        (splicing-let-syntax () (loop (- n 1) (+ count 1))))))

(test-equal "definitions in an operand splice stay local" '(inner outer)
  (let ((x 'outer))
    (list (splicing-let-syntax () (define x 'inner) x) x)))
(test-equal "definitions in an if branch splice stay local" '(inner outer)
  (let ((x 'outer))
    (list (if #t (splicing-let-syntax () (define x 'inner) x) #f) x)))
(test-equal "a definition initializer is an expression context" '(inner outer)
  (let ((x 'outer))
    (define result (splicing-let-syntax () (define x 'inner) x))
    (list result x)))
(test-equal "an unquote is an expression context" '(inner outer)
  (let ((x 'outer))
    `(,(splicing-letrec-syntax () (define x 'inner) x) ,x)))
(test-equal "a syntax definition in an operand stays local" '(inner outer)
  (let-syntax ((keyword (syntax-rules () ((_) 'outer))))
    (list (splicing-let-syntax ()
            (define-syntax keyword (syntax-rules () ((_) 'inner)))
            (keyword))
          (keyword))))
(test-equal "a local keyword can generate definitions through a begin" 56
  (let ()
    (splicing-let-syntax ()
      (begin
        (define-syntax def (syntax-rules () ((_ name) (define name 56))))
        (def answer)))
    answer))
(test-equal "a macro defined in a splice contributes later variable bindings" '(56 56)
  (let ()
    (define (earlier) if)
    (splicing-let-syntax ()
      (define-syntax def (syntax-rules () ((_ name) (define name 56))))
      (def if))
    (list (earlier) if)))
(test-equal "an escaping definer contributes later variable bindings" '(57 57)
  (let ()
    (define (earlier) if)
    (splicing-let-syntax ()
      (define-syntax def (syntax-rules () ((_ name) (define name 57)))))
    (def if)
    (list (earlier) if)))
(test-equal "a source binding can shadow the splicing form" 57
  (let ((splicing-let-syntax (lambda (x) x))) (splicing-let-syntax 57)))
(test-equal "recursive keywords survive in an escaping transformer" '(a b)
  (let ()
    (splicing-letrec-syntax
        ((collect (syntax-rules () ((_) '()) ((_ x xs ...) (cons 'x (collect xs ...))))))
      (define-syntax answer (syntax-rules () ((_) (collect a b)))))
    (answer)))
(splicing-let-syntax ((def (syntax-rules () ((_ name) (define name 58)))))
  (def top-level-spliced-value))
(test-equal "a top-level splice defines a public variable" 58 top-level-spliced-value)
(splicing-let-syntax ((helper (syntax-rules () ((_) 59))))
  (define-syntax top-level-spliced-macro (syntax-rules () ((_) (helper)))))
(test-equal "a top-level macro retains its private helper" 59 (top-level-spliced-macro))

(test-error "a splice requires a binding list"
  (eval '(splicing-let-syntax) (environment '(scheme base) '(srfi 188))))
(test-error "a splice requires proper bindings"
  (eval '(splicing-letrec-syntax (bad) 1) (environment '(scheme base) '(srfi 188))))
(test-error "a splice requires a keyword identifier"
  (eval '(splicing-let-syntax ((1 (syntax-rules () ((_) 2)))) 3)
        (environment '(scheme base) '(srfi 188))))
(test-error "an expression splice requires a body"
  (eval '(list (splicing-let-syntax ())) (environment '(scheme base) '(srfi 188))))
(test-error "ordinary let-syntax still rejects an empty body"
  (eval '(let-syntax ()) (environment '(scheme base))))
(test-error "ordinary letrec-syntax still rejects an empty body"
  (eval '(letrec-syntax ()) (environment '(scheme base))))

;; The compatibility facade is Patina's; Chibi supplies the SRFI instead.
(cond-expand
 (patina
  (test-equal "rnrs base re-exports the splicing forms" 53
    (eval '(let () (let-syntax () (define answer 53)) answer)
          (environment '(rnrs base))))
  (test-equal "r6rs base exports the shared splicing forms" '(50 51)
    (eval '(list (let () (let-syntax () (define answer 50)) answer)
                 (let () (letrec-syntax () (define answer 51)) answer))
          (environment '(r6rs base))))
  (test-equal "r6rs base accepts empty local syntax bodies" 52
    (eval '(let () (let-syntax ()) (letrec-syntax ()) 52)
          (environment '(r6rs base))))))
(test-end)
