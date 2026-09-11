;; `(scheme eval)` — `eval` and `environment`, R7RS §6.12 — and the two R5RS
;; environment procedures `(scheme r5rs)` carries.
;;
;; Migrated from `crates/patina-tests/tests/scheme_eval.rs` (#193 Phase 2),
;; which is deleted, joining the one row Larceny family 9 had already moved
;; here from `larceny_families.rs` (Phase 1). That Rust file built a
;; `TreeWalkInterpreter` by hand for every test, so **none of its 34 tests had
;; ever run on the VM**; every row here runs on both backends.
;;
;; The error rows assert only that an error is raised. The two backends word
;; these errors differently — the VM prefixes "Invalid syntax:" to several —
;; and wording is not an interface; the one row that cares what the message
;; says is scoped to Patina and asks only for the word that matters.
;;
;; Divergences are recorded in `DIVERGENCES.tsv`, not restated here.

(import (scheme base) (scheme eval) (scheme inexact) (scheme r5rs) (srfi 64))

(test-begin "eval")

;; ── eval in an environment ─────────────────────────────────────────────────

(test-equal "eval evaluates an expression in an environment" '(21 15 1024)
  (list (eval '(* 7 3) (environment '(scheme base)))
        (eval '(+ 1 2 3 4 5) (environment '(scheme base)))
        (eval '(expt 2 10) (environment '(scheme base)))))

;; R7RS §6.12's second example, with `(scheme base)` in place of the
;; report's `(null-environment 5)`, which holds no procedures to apply.
(test-equal "a procedure eval returns works outside it" '(25 20)
  (list (let ((f (eval '(lambda (x) (* x x)) (environment '(scheme base)))))
          (f 5))
        (let ((f (eval '(lambda (f x) (f x x)) (environment '(scheme base)))))
          (f + 10))))

(test-equal "eval handles the derived forms" '(1 30 25)
  (list (eval '(if #t 1 2) (environment '(scheme base)))
        (eval '(let ((x 10) (y 20)) (+ x y)) (environment '(scheme base)))
        (eval '(let ((square (lambda (x) (* x x))))
                 (+ (square 3) (square 4)))
              (environment '(scheme base)))))

(test-equal "eval reaches list procedures and quote" '(1 (2 4 6) (a b c))
  (list (eval '(car (cons 1 2)) (environment '(scheme base)))
        (eval '(map (lambda (x) (* x 2)) '(1 2 3)) (environment '(scheme base)))
        (eval '(quote (a b c)) (environment '(scheme base)))))

(test-equal "an environment can import any library, and several" '(0.0 1024.0)
  (list (inexact (eval '(sin 0) (environment '(scheme inexact))))
        (eval '(+ (expt 2 10) (inexact (sin 0)))
              (environment '(scheme base) '(scheme inexact)))))

;; `(environment)` with no import sets binds nothing, so `if` is unbound in it.
;;
;; The Rust row once asserted 42 here — "special forms should work even in an
;; empty environment" — which was true only because the desugarer recognized
;; keywords by spelling wherever nothing was bound. Keywords are ordinary
;; bindings now, and an environment that imports nothing has none of them.
;; chibi and Gauche both report `if` as undefined.
(test-error "an empty environment has no syntax" #t
  (eval '(if #t 42 0) (environment)))

;; …and the same expression works once something exporting `if` is imported,
;; so the failure is about the environment, not about `eval`.
(test-equal "the same expression with (scheme base) imported" 42
  (eval '(if #t 42 0) (environment '(scheme base))))

;; R7RS §6.12: "The bindings of the environment represented by the specifier
;; are immutable, as is the environment itself", and its own example gives
;; `(eval '(define foo 32) (environment '(scheme base)))` as "error is
;; signaled".
(test-error "an environment is immutable" #t
  (eval '(define foo 32) (environment '(scheme base))))

(test-error "eval needs an environment specifier" #t
  (eval '(+ 1 2) 42))

;; ── What is Patina's own ───────────────────────────────────────────────────

;; How an environment specifier prints is the implementation's business; this
;; is Patina's, for every constructor of one.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "an environment specifier prints as #<environment>"
  '("#<environment>" "#<environment>" "#<environment>" "#<environment>")
  (map (lambda (env)
         (let ((port (open-output-string)))
           (write env port)
           (get-output-string port)))
       (list (environment '(scheme base))
             (environment)
             (null-environment 5)
             (scheme-report-environment 5))))

;; The immutability error names what went wrong. The Rust row asked for the
;; word "immutable" in the message and nothing more, because the two
;; backends' messages differ around it.
(cond-expand (patina) (else (test-skip 1)))
(test-assert "the immutability error says so"
  (let ((message (guard (e ((error-object? e) (error-object-message e)))
                   (eval '(define foo 32) (environment '(scheme base)))
                   "no error")))
    (let loop ((i 0))
      (cond ((> (+ i 9) (string-length message)) #f)
            ((string=? (substring message i (+ i 9)) "immutable") #t)
            (else (loop (+ i 1)))))))

;; ── The R5RS environments ──────────────────────────────────────────────────
;;
;; R7RS keeps `null-environment` and `scheme-report-environment` in
;; `(scheme r5rs)`. Version 5 must be supported; others may be.

(test-equal "null-environment 5 has the syntactic keywords" '(1 42 (a b c))
  (list (eval '(if #t 1 2) (null-environment 5))
        (eval '((lambda (x) x) 42) (null-environment 5))
        (eval '(quote (a b c)) (null-environment 5))))

;; chibi raises this one outside every handler — the unbound `+` is reported
;; while compiling the `eval`'d form and ends the program — so the row is
;; skipped there rather than costing the oracle the whole file.
(cond-expand (chibi (test-skip 1)) (else))
(test-error "null-environment 5 has no procedures" #t
  (eval '(+ 1 2) (null-environment 5)))

(test-equal "scheme-report-environment 5 has the R5RS procedures"
  '(6 1 (2 4 6) 100 (#t #t #t) "hello world")
  (let ((env (scheme-report-environment 5)))
    (list (eval '(+ 1 2 3) env)
          (eval '(car (cons 1 2)) env)
          (eval '(map (lambda (x) (* x 2)) '(1 2 3)) env)
          (eval '(let ((x 10)) (* x x)) env)
          (eval '(list (number? 42) (string? "hello") (null? '())) env)
          (eval '(string-append "hello" " " "world") env))))

;; "If version is neither 5 nor another value supported by the
;; implementation, an error is signaled." Patina supports only 5.
(test-error "null-environment rejects a version it does not support" #t
  (null-environment 6))

(test-error "scheme-report-environment rejects a version it does not support" #t
  (scheme-report-environment 7))

;; R7RS leaves defining into an R5RS environment unspecified — "both the
;; environment and the bindings it contains may be immutable" — and on
;; Patina they are, like `environment`'s. chibi refuses too, but raises
;; "immutable binding" outside every handler, like the unbound `+` above, so
;; both rows are skipped there.
(cond-expand (chibi (test-skip 1)) (else))
(test-error "defining into scheme-report-environment is refused" #t
  (eval '(define x 10) (scheme-report-environment 5)))

(cond-expand (chibi (test-skip 1)) (else))
(test-error "defining into null-environment is refused" #t
  (eval '(define x 10) (null-environment 5)))

;; ── A known-open defect ────────────────────────────────────────────────────

;; **A known-open defect, quarantined so it announces its own fix.**
;; `(prefix (only …) …)` is an ordinary import set, and `environment` should
;; accept whatever `import` accepts. Ours rejects it: "library name component
;; must be a symbol, got pair". chibi and Gauche both answer `1`, so this is
;; our gap and not a difference of opinion.
;;
;; The `.rs` row asserted only that this *errors*, with a comment telling the
;; next reader to replace the assertion by hand when it stops. SRFI 64 has the
;; mechanism: `test-expect-fail` plus the **right** answer, so the row is xfail
;; today and xpass the moment it is fixed — and the driver fails the build on
;; xpass, which is what makes a quarantine retire itself rather than wait to be
;; noticed.
;;
;; **The program is not the `.rs` one, because that one could never pass.** It
;; used `(eval '(p:car '(1 2)) …)`, and the environment it builds holds exactly
;; one binding — `p:car`. `quote` is not in it, so `'(1 2)` cannot be
;; evaluated: chibi and Gauche both get past `environment` and then fail with
;; "invalid application: (1 2)", because `(quote (1 2))` is an application of an
;; unbound `quote`. The documented fixed answer of `1` was unreachable. Built
;; from `cons` instead, the row means what it says, and the oracles reach the
;; answer the quarantine is waiting for.
;;
;; Scoped, because the expectation is about our gap alone.
(cond-expand (patina (test-expect-fail 1)) (else))
(test-equal "environment accepts a nested import set" 1
  (eval '(p:car (p:cons 1 2))
        (environment '(prefix (only (scheme base) car cons) p:))))

(test-end)
