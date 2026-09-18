;; Upstream's own s16vector suite for `(srfi 160 s16)`, wrapped as a
;; `(srfi 160 test)` library so `upstream_srfi_suites.rs` can run it.
;;
;; The body is `shared-tests.scm`, verbatim from the SRFI's distribution, and
;; the imports are the ones its `chibi-tests.scm` driver supplies. Only the
;; wrapper is ours: upstream ships the suite as a bare script that `include`s
;; the shared body, and the harness here needs a library exporting
;; `run-tests`.
;;
;; Upstream tests s16 alone, and says why in its own comment: "if one vector
;; type works, they all work" — the twelve libraries are `sed`-expanded from
;; one template, so a defect in the template shows in every one of them.
;; `crates/patina-tests/tests/scheme/srfi/homogeneous-vectors.scm` is where
;; the other eleven are exercised.

(define-library (srfi 160 test)
  (import (scheme base)
          (scheme write)
          (chibi test)
          (srfi 128)
          (srfi 160 s16))
  (export run-tests)
  (begin
    (define (sub1 x) (- x 1))
    (define (run-tests)
      (include "shared-tests.scm"))))
