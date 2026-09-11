;; `apply` — R7RS §6.10.
;;
;; Migrated from `crates/patina-tests/tests/compliance/control.rs` (#193),
;; which is deleted and whose other rows went by subject: `values` and
;; `call-with-values` to `control/values.scm`, handlers and the re-entered
;; `dynamic-wind` to `control/cps-features.scm`, and `guard` to
;; `control/guard.scm`.

(import (scheme base) (srfi 64))

(test-begin "apply")

(test-equal "apply a procedure to a list" 7 (apply + (list 3 4)))
(test-equal "apply with arguments before the list" 10 (apply + 1 2 (list 3 4)))
(test-equal "apply to the empty list" 0 (apply + (list)))
(test-equal "apply with several leading arguments" 120 (apply * 2 3 (list 4 5)))

;; The report's own example.
(define compose
  (lambda (f g)
    (lambda args
      (f (apply g args)))))
(test-equal "apply inside a variadic lambda, the report's compose" -87
  ((compose - +) 12 75))

(test-end)
