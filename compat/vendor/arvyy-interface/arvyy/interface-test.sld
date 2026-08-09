(define-library
  (arvyy interface-test)
  (import (arvyy interface)
          (scheme base))
  (export run-tests)
  (cond-expand
    ((library (srfi 64))
     (import (srfi 64)))
    (chibi
      (import (rename (except (chibi test) test-equal)
                      (test test-equal)))))

  (begin
    (define (run-tests)
      (test-begin "interface")

      (test-group "Direct implementation"

                  (define-interface 
                    make-foo-impl
                    (proc0)
                    (proc1 arg1)
                    (procn . args)
                    (procn1+ arg . args))

                  (define foo
                    (make-foo-impl
                      ((proc0) `("proc0"))
                      ((proc1 arg1) `("proc1" ,arg1))
                      ((procn . args) `("procn" ,args))
                      ((procn1+ arg . args) `("procn1+" ,arg ,args))))

                  (test-equal '("proc0") (proc0 foo))
                  (test-equal '("proc1" 1) (proc1 foo 1))
                  (test-equal '("procn" (1 2)) (procn foo 1 2))
                  (test-equal '("procn" ()) (procn foo))
                  (test-equal '("procn1+" 1 ()) (procn1+ foo 1))
                  (test-equal '("procn1+" 1 (2 3)) (procn1+ foo 1 2 3)))

      (test-group "Call-compatible implementation"
                  (define-interface
                    make-foo-impl
                    (procn a b c d))
                  (define foo
                    (make-foo-impl
                      ((procn . args) `("procn" ,args))))

                  (test-equal '("procn" (1 2 3 4)) (procn foo 1 2 3 4)))

      (test-end))))
