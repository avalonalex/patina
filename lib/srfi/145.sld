;; SRFI 145: assumptions.
;;
;; The SRFI's own first sample implementation, with its `cond-expand` resolved
;; rather than carried. Upstream offers two: one that reports a violated
;; assumption, and one whose whole body is `((assume obj . _) obj)` — which
;; discards the check. The first is taken, because a library that silently
;; drops its assertions is worse than no library.
;;
;; Upstream's `debug` cond-expand chooses between `error` and `(car 0)`, a
;; deliberate crash for implementations that optimize on the promise. Patina
;; has no `debug` feature and nothing to gain from crashing uninformatively,
;; so the reporting branch is always taken and the dead one is not carried.
;; The message and irritants are upstream's.
;;
;; Bundled for `(srfi 146)` — see lib/srfi/PROVENANCE.md.

(define-library (srfi 145)
  (import (scheme base))
  (export assume)
  (begin
    (define-syntax assume
      (syntax-rules ()
        ((assume expression message ...)
         (or expression
             (error "invalid assumption" (quote expression) (list message ...))))))))
