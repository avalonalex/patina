;; Exception handling macros for (scheme base)
;;
;; R7RS Section 4.2.7 - Exception handling

;; (guard (var clause ... [clause]) body1 body2 ...)
;;
;; The guard form establishes a handler that catches exceptions raised
;; during evaluation of the body expressions. If an exception is raised,
;; the clauses are evaluated like cond clauses, with var bound to the
;; raised object. If no clause matches and there's no else clause,
;; the exception is re-raised.

(define-syntax guard
  (syntax-rules (else)
    ;; Case with else clause - always matches
    ((guard (var clause ... (else result1 result2 ...))
       body1 body2 ...)
     (call-with-current-continuation
       (lambda (guard-k)
         (with-exception-handler
           (lambda (condition)
             (guard-k
               (let ((var condition))
                 (cond clause ... (else result1 result2 ...)))))
           (lambda () body1 body2 ...)))))

    ;; Case without else clause - may re-raise
    ;; If no clause matches, the exception is re-raised
    ((guard (var clause ...)
       body1 body2 ...)
     (call-with-current-continuation
       (lambda (guard-k)
         (with-exception-handler
           (lambda (condition)
             (let ((var condition))
               ;; Evaluate clauses like cond; if #f, re-raise
               (guard-k
                 (cond
                   clause ...
                   (else (raise condition))))))
           (lambda () body1 body2 ...)))))))
