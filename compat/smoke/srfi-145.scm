(import (scheme base) (prefix (srfi 145) s:) (patina compat smoke))
(check-equal "true assumptions return their value" #t (s:assume #t))
(define marker (list 'marker))
(check-equal "success preserves the returned object" #t
  (eq? marker (s:assume marker "message")))
(check-equal "zero and the empty list are true values" '(0 ())
  (list (s:assume 0) (s:assume '())))
(define calls 0)
(check-equal "the successful expression is evaluated once" '(1 1)
  (let ((result (s:assume (begin (set! calls (+ calls 1)) calls))))
    (list result calls)))
(check-equal "diagnostic expressions are lazy on success" 'ok
  (s:assume 'ok (error "diagnostic should not run")))
(check-equal "local names do not capture the assumption machinery" 'safe
  (let ((or #f) (fatal-error #f) (expression #f)) (s:assume 'safe)))
;; SRFI 145 permits an implementation not to signal false assumptions.
;; These two checks measure this pinned implementation's explicit error path.
;; https://srfi.schemers.org/srfi-145/srfi-145.html
(check-error "the pinned implementation signals a false assumption" (s:assume #f))
(check-equal "failure evaluates its expression and diagnostic once" '(#t 1 1)
  (let ((expressions 0) (messages 0))
    (let ((raised
            (guard (ex (else #t))
              (s:assume (begin (set! expressions (+ expressions 1)) #f)
                        (begin (set! messages (+ messages 1)) "reason"))
              #f)))
      (list raised expressions messages))))
(smoke-finish)
