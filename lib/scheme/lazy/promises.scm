;; promises.scm - Macros for lazy evaluation
;;
;; This file provides the delay and delay-force macros for lazy evaluation.
;; These macros work with the promise primitives (force, promise?, make-promise)
;; implemented in Rust.

;; delay - Create a lazy promise
;;
;; (delay <expression>) returns a promise which, when forced, will evaluate
;; <expression> and return its value. The value is cached so subsequent forces
;; return the same value without re-evaluation.
;;
;; Example:
;;   (define p (delay (+ 1 2)))
;;   (force p)  ; => 3
;;   (force p)  ; => 3 (cached, doesn't re-evaluate)
;;
;; R7RS 7.3: (delay e) is (delay-force (make-promise #t e)) — the value is
;; wrapped in a *done* promise, so forcing (delay p) yields the promise p
;; itself rather than forcing through it; forcing through is delay-force's
;; job. %make-forced-promise is that unconditional wrap (make-promise, the
;; procedure, returns a promise argument as-is and would collapse the two).
(define-syntax delay
  (syntax-rules ()
    ((delay expression)
     (%make-delayed-promise (lambda () (%make-forced-promise expression))))))

;; delay-force - Create a lazy promise with proper tail recursion
;;
;; (delay-force <expression>) is used for tail-recursive lazy computations.
;; The <expression> should evaluate to a promise. When forced, the result
;; promise is forced recursively until a non-promise value is obtained.
;;
;; This is essential for lazy algorithms that would otherwise build up
;; chains of promises.
(define-syntax delay-force
  (syntax-rules ()
    ((delay-force expression)
     (%make-delayed-promise (lambda () expression)))))
