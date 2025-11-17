;; (scheme lazy) extras - Macros for lazy evaluation
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
(define-syntax delay
  (syntax-rules ()
    ((delay expression)
     (%make-delayed-promise (lambda () expression)))))

;; delay-force - Create a lazy promise with proper tail recursion
;;
;; (delay-force <expression>) is used for tail-recursive lazy computations.
;; The <expression> should evaluate to a promise. When forced, the result
;; promise is forced recursively until a non-promise value is obtained.
;;
;; This is essential for lazy algorithms that would otherwise build up
;; chains of promises:
;;
;; Bad (builds promise chain):
;;   (define (lazy-filter pred lst)
;;     (delay
;;       (if (null? lst)
;;           '()
;;           (let ((head (car lst))
;;                 (tail (cdr lst)))
;;             (if (pred head)
;;                 (cons head (lazy-filter pred tail))
;;                 (force (lazy-filter pred tail)))))))  ; Builds promise chain!
;;
;; Good (constant space):
;;   (define (lazy-filter pred lst)
;;     (delay-force
;;       (if (null? lst)
;;           (delay '())
;;           (let ((head (car lst))
;;                 (tail (cdr lst)))
;;             (if (pred head)
;;                 (delay (cons head (lazy-filter pred tail)))
;;                 (lazy-filter pred tail))))))  ; Tail call, no chain!
(define-syntax delay-force
  (syntax-rules ()
    ((delay-force expression)
     (%make-delayed-promise (lambda () expression)))))
