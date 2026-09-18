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

;; `shared-tests.scm` ends with `(test-exit)`, which calls `exit`. The suites
;; here run *in-process*, so that call terminated the whole `cargo test`
;; process: every test scheduled after this one silently never ran, libtest
;; printed no summary, and cargo still exited 0 — the lane looked green while
;; skipping tests. Measured: 7 of this binary's tests were erased, and 36
;; across `cargo test --all`, whose 1354 passing tests had been 1318. The
;; harness's own two self-checks were among them. See #396.
;;
;; `test-exit` is excluded from the `(chibi test)` import and shadowed below,
;; so the body's last line reports and returns instead of exiting. The suite
;; body itself stays verbatim, which is the point: the shadow lives in this
;; wrapper, which is ours.
;;
;; `test-exit` is only ever the *last* thing a suite does, and its return
;; value is unused, so dropping the `(exit)` loses nothing: the harness reads
;; the failure count from the framework afterwards, which is how it has always
;; decided whether the suite passed — `(exit)`'s status was never consulted.
;;
;; What the shadow does keep is the *other* half of chibi's `test-exit`, which
;; warns when it is reached with a test group still open. That matters because
;; this body is re-vendored verbatim, and a dropped or misnested `test-end`
;; would leave assertions untallied while the assertion floor — which only
;; catches a shortfall — could still be met. `warning` is not exported, so
;; this raises instead, which is the stronger answer for a test lane.
(define-library (srfi 160 test)
  (import (scheme base)
          (scheme write)
          (except (chibi test) test-exit)
          (srfi 128)
          (srfi 160 s16))
  (export run-tests)
  (begin
    (define (sub1 x) (- x 1))
    (define (test-exit)
      (if (current-test-group)
          (error "test-exit reached with an unfinished test group"
                 (test-group-name (current-test-group)))))
    (define (run-tests)
      (include "shared-tests.scm"))))
