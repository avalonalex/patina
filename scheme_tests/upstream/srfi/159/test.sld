;; Upstream's own suite for `(srfi 159)`, wrapped as a `(srfi 159 test)`
;; library so `upstream_srfi_suites.rs` can run it.
;;
;; `shared-tests.scm` is `contrib/duy-nguyen/test.scm` from the SRFI's
;; distribution with three things lifted out of it and nothing else changed:
;; its leading `import` form and the `cond-expand` that picks the test
;; framework, both of which belong in a library declaration rather than an
;; `include`, and its trailing bare `(run-tests)` call, since the harness is
;; what calls it. All 316 assertions and the `test-pretty` macro are
;; upstream's, untouched.
;;
;; The framework `cond-expand` is reproduced here with one change forced by
;; the move: upstream's `larceny` branch writes its `define` bare, which is
;; legal in the program body it came from but not in a library declaration,
;; so it is wrapped in `(begin ...)`. Its `else` branch takes `(chibi test)`,
;; which is what this harness supplies; the `larceny` branch is never
;; selected here and is kept so the file still reads as upstream's.

(define-library (srfi 159 test)
  (export run-tests)
  (import (scheme base) (scheme char) (scheme read) (scheme file)
          (only (srfi 1) circular-list)
          (srfi 159))
  (cond-expand
   (larceny
    (import (srfi 64))
    (begin
      (define (test expected actual)
        (test-equal expected actual))))
   (else
    (import (chibi test))))
  (include "shared-tests.scm"))
