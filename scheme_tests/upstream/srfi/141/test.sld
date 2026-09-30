;; Chibi's tests/division-tests.scm at
;; bb9b3215e52bd29cecdfa3ce37cd97721f0c2cc0, BSD-3-Clause (COPYING).
;; Only the import form moved into this wrapper; all assertions are unchanged.
;; Upstream has a script; the Patina harness calls run-tests after installing
;; its counting reporter. The suite never calls exit.
(define-library (srfi 141 test)
  (import (chibi test) (scheme base) (scheme division))
  (export run-tests)
  (begin
    (define (run-tests)
      (include "division-tests.scm"))))
