;; #424: definitions spliced into a library, and private expansion-time
;; bindings retained by exported transformers. Chibi 0.12 cannot load inline
;; libraries; these library bodies were also checked there as .sld files.
;; Gauche 0.9.15 has no (srfi 188). Both limitations are registered.
(define-library (probe splice)
  (import (scheme base) (srfi 188))
  (export answer recur value another make-answer generated-in-library
          define-spliced define-rec-spliced)
  (begin
    (splicing-let-syntax ((helper (syntax-rules () ((_) 53))))
      (define value 54)
      (define-syntax answer (syntax-rules () ((_) (helper)))))
    (splicing-letrec-syntax
        ((helper (syntax-rules () ((_ 0) 55) ((_ n) (helper 0)))))
      (define-syntax recur (syntax-rules () ((_) (helper 1)))))
    (splicing-let-syntax ((helper (syntax-rules () ((_) 60))))
      (define-syntax another (syntax-rules () ((_) (helper)))))
    (splicing-let-syntax ((helper (syntax-rules () ((_) 61))))
      (define-syntax make-answer
        (syntax-rules ()
          ((_ name) (define-syntax name (syntax-rules () ((_) (helper))))))))
    (make-answer generated-in-library)
    (define-syntax define-spliced
      (syntax-rules () ((_ name) (splicing-let-syntax () (define name 62)))))
    (define-syntax define-rec-spliced
      (syntax-rules () ((_ name) (splicing-letrec-syntax () (define name 63)))))))
(define-library (probe reexport)
  (import (probe splice))
  (export (rename answer renamed-answer) recur value))
(import (scheme base) (scheme eval) (srfi 64) (probe splice)
        (prefix (probe reexport) reexport:))
(test-begin "splicing-libraries")
(define-syntax helper (syntax-rules () ((_) 'caller)))
(test-equal "a library exports a spliced variable" 54 value)
(test-equal "an exported macro retains a private keyword" 53 (answer))
(test-equal "an exported macro retains a recursive keyword" 55 (recur))
(test-equal "same-spelled private keywords keep separate bindings" '(53 60)
  (list (answer) (another)))
(test-equal "a renamed re-export keeps its private keyword" 53 (reexport:renamed-answer))
(make-answer generated-answer)
(test-equal "a generated macro retains the library's private keyword" 61 (generated-answer))
(test-equal "a macro generated inside its library retains the helper" 61 (generated-in-library))
(test-equal "private keywords do not replace the caller's binding" 'caller (helper))
(test-equal "local shadowing does not capture exported helpers" '(53 55 61)
  (let-syntax ((helper (syntax-rules () ((_) 'local))))
    (list (answer) (recur) (generated-answer))))
(test-equal "a library-generated splice binds before earlier references" '(62 62)
  (let ()
    (define (earlier) if)
    (define-spliced if)
    (list (earlier) if)))
(test-equal "a library-generated recursive splice binds before earlier references" '(63 63)
  (let ()
    (define (earlier) quote)
    (define-rec-spliced quote)
    (list (earlier) quote)))
(test-error "a private keyword is not a library export"
  (environment '(only (probe splice) helper)))
(test-end)
