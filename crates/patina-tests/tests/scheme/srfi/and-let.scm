;; SRFI 2 `and-let*` and SRFI 145 `assume`.
;;
;; Both are bundled for `(srfi 146)`, and neither has an upstream suite to
;; register in `upstream_srfi_suites.rs` — SRFI 2 predates `syntax-rules` and
;; its reference implementation is a low-level macro the document does not
;; carry in portable form, and SRFI 145 ships none at all. So this file is
;; what stands in for one, and the rows are written from each SRFI's own text
;; rather than from what our implementation happens to do.
;;
;; `and-let*` is the one worth testing properly: it has three claw forms that
;; look alike and behave differently, and a set of edge cases the SRFI spells
;; out explicitly (an empty claw list, a claw list with no body) that a
;; plausible-looking `syntax-rules` version gets wrong.

(import (scheme base) (srfi 2) (srfi 145) (srfi 64))

(test-begin "and-let")

;; ---------------------------------------------------------------------------
;; SRFI 2: the three claw forms
;; ---------------------------------------------------------------------------

;; (var expr) binds, and the binding is in scope for every later claw.
(test-equal "a binding claw binds for the claws after it" 8
  (and-let* ((x 2) (y (* x 3))) (+ x y)))
(test-equal "a binding claw binds for the body" 2
  (and-let* ((x 2)) x))

;; (expr) tests without binding.
(test-equal "a parenthesised test claw does not bind" 7
  (and-let* ((x 7) ((> x 0))) x))
(test-assert "a parenthesised test claw can fail the form"
  (not (and-let* ((x 7) ((< x 0))) x)))

;; A bare expression tests without binding. The SRFI restricts this to a
;; variable reference; accepting any expression is the extension every
;; implementation makes.
(test-equal "a bare claw tests without binding" 3
  (let ((v 3)) (and-let* (v) v)))
(test-assert "a bare false claw fails the form"
  (not (let ((v #f)) (and-let* (v) 'unreached))))

;; ---------------------------------------------------------------------------
;; SRFI 2: the cases the document spells out
;; ---------------------------------------------------------------------------

(test-equal "no claws and no body is #t" #t (and-let* ()))
(test-equal "no claws with a body is the body" 2 (and-let* () 1 2))

;; A trailing claw with no body yields the claw's own value, not #t. This is
;; the row that separates a correct implementation from a plausible one.
(test-equal "a lone binding claw with no body is its value" 5
  (and-let* ((x 5))))
(test-equal "a lone test claw with no body is its value" 5
  (and-let* ((5))))
(test-equal "a lone bare claw with no body is its value" 5
  ;; A variable, not a literal: the SRFI restricts the bare claw to a variable
  ;; reference, and Gauche enforces that — it rejects `(and-let* (5))` as
  ;; malformed. Patina and chibi accept the literal, but a row asserting the
  ;; extension would be testing our leniency rather than the SRFI.
  (let ((v 5)) (and-let* (v))))

;; ---------------------------------------------------------------------------
;; SRFI 2: short-circuiting
;; ---------------------------------------------------------------------------
;; A false claw must stop evaluation dead — nothing after it runs, and the
;; form is #f rather than an error.

(test-assert "a false claw stops the claws after it"
  (not (and-let* ((x #f) (y (error "must not be evaluated"))) y)))
(test-assert "a false claw stops the body"
  (not (and-let* ((x #f)) (error "must not be evaluated"))))
(test-equal "evaluation stops at the first false claw" 1
  (let ((count 0))
    (and-let* ((a (begin (set! count (+ count 1)) #t))
               (b #f)
               (c (begin (set! count (+ count 1)) #t)))
      'unreached)
    count))

;; A test claw's expression is evaluated once, not once per use.
(test-equal "a test claw evaluates its expression once" 1
  (let ((count 0))
    (and-let* (((begin (set! count (+ count 1)) #t))) 'ok)
    count))

;; ---------------------------------------------------------------------------
;; SRFI 2: the body is a body
;; ---------------------------------------------------------------------------

(test-equal "the body takes several expressions and yields the last" 3
  (and-let* ((x 1)) 1 2 3))
(test-equal "the body admits an internal define" 4
  (and-let* ((x 2)) (define y (* x 2)) y))

;; ---------------------------------------------------------------------------
;; SRFI 145: assume
;; ---------------------------------------------------------------------------
;; The SRFI leaves behaviour undefined when an assumption does not hold, and
;; offers two sample implementations — one reporting, one discarding the check
;; entirely. We take the reporting one, so these rows pin that choice rather
;; than the standard.

(test-equal "a held assumption yields a true value and runs on" 16
  (let ()
    (define (f x) (assume (exact-integer? x) "f takes integer arguments" x) (* x x))
    (f 4)))
(test-assert "a violated assumption raises"
  (guard (e (#t #t))
    (assume #f "this assumption does not hold")
    #f))
(test-assert "a violated assumption raises with no message given"
  (guard (e (#t #t))
    (assume #f)
    #f))
(test-assert "the raised object is an error object naming the assumption"
  (guard (e ((error-object? e)
             (and (string? (error-object-message e))
                  ;; The irritants carry the expression that failed, so a
                  ;; report can say which assumption it was.
                  (pair? (error-object-irritants e)))))
    (assume (= 1 2) "one is not two")
    #f))

(test-end)
