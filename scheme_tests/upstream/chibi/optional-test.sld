
(define-library (chibi optional-test)
  (import (scheme base) (chibi optional))
  ;; Patina adaptation: upstream's cond-expand resolved by hand to its chibi
  ;; branch. Patina bundles (chibi test) but does not advertise the `chibi`
  ;; feature, so the else branch's inline framework shim would be chosen —
  ;; and it neither counts failures nor reports through
  ;; current-test-reporter; its own test-error also lacks the two-argument
  ;; form this suite's body uses, so the shim cannot even expand here.
  ;; Test bodies untouched. See scheme_tests/upstream/README.md.
  (import (chibi test))
  (export run-tests)
  (begin
    (define (run-tests)
      (test-begin "optional")
      (test '(0 11 12)
          (let-optionals '(0) ((a 10) (b 11) (c 12))
            (list a b c)))
      (test '(0 11 12)
          ((opt-lambda ((a 10) (b 11) (c 12))
             (list a b c))
           0))
      (test '(0 11 12)
          ((opt-lambda (a (b 11) (c 12))
             (list a b c))
           0))
      (test '(0 1 (2 3 4))
          (let-optionals* '(0 1 2 3 4) ((a 10) (b 11) . c)
            (list a b c)))
      (test '(0 1 (2 3 4))
          (let-optionals '(0 1 2 3 4) ((a 10) (b 11) . c)
            (list a b c)))
      (cond-expand
       (gauche)     ; gauche detects this at compile-time, can't catch
       (else (test-error '(0 11 12)
                         ((opt-lambda (a (b 11) (c 12))
                            (list a b c))))))
      (let ()
        (define-opt (f a (b 11) (c 12))
          (list a b c))
        (cond-expand
         (gauche)
         (else
          (test-error (f))))
        (test '(0 11 12) (f 0))
        (test '(0 1 12) (f 0 1))
        (test '(0 1 2) (f 0 1 2))
        (test '(0 1 2) (f 0 1 2 3)))
      (test-end))))
