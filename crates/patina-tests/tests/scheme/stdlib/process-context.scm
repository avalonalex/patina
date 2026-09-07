;; `(scheme process-context)` — R7RS §6.14.
;;
;; Migrated from `crates/patina-tests/tests/scheme_process_context.rs` (#193
;; Phase 1). 12 rows there, 11 here — two were the same assertion written twice
;; — and one added, which the `.rs` file had written out and commented.
;;
;; **These rows describe shapes, not values.** The process environment is not
;; ours to fix: `PATH`'s contents differ per machine, `command-line`'s first
;; element is explicitly implementation-dependent (R7RS §6.14), and the
;; environment block is whatever the caller exported. So each row asserts the
;; type or the arity of what comes back, and the two rows that name a variable
;; use `PATH`, which every platform this runs on defines, and one that nothing
;; defines.

(import (scheme base) (scheme process-context) (srfi 64))

(test-begin "process-context")

;; ── command-line ────────────────────────────────────────────────────────────

(test-equal "command-line returns a list" #t (list? (command-line)))

;; R7RS §6.14: "the first element is an implementation-dependent string
;; identifying the calling program" — it may even be empty, so the row says it
;; is a string and that there is one, not what it holds.
(test-equal "its first element is a string" #t (string? (car (command-line))))
(test-equal "and there is always at least one" #t (>= (length (command-line)) 1))

;; ── get-environment-variable ────────────────────────────────────────────────

;; The `.rs` file asserted this twice, in two sections, with the same program;
;; once is enough. `PATH` is the variable to use because it is the one every
;; platform defines — the row is about the return type, not the value.
(test-equal "a variable that exists reads back as a string" #t
  (string? (get-environment-variable "PATH")))

(test-equal "one that does not is #f" #f
  (get-environment-variable "PATINA_NONEXISTENT_VAR_12345"))

;; A non-string name is an error, and a catchable one.
;;
;; This row existed in the `.rs` file — commented out, with the reason "Can't
;; test type error without guard, which isn't implemented yet". `guard` has been
;; implemented for a long time, so the note outlived its premise and the row
;; stayed dark. Written out and enabled here.
(test-equal "a non-string name is a catchable error" 'type-error
  (guard (e (#t 'type-error)) (get-environment-variable 123)))

;; ── get-environment-variables ───────────────────────────────────────────────

;; An association list of strings. Every row guards the empty case: R7RS does
;; not promise the block is non-empty, and a row that indexed into it blindly
;; would be asserting about the machine rather than about the procedure.
(test-equal "returns a list" #t (list? (get-environment-variables)))

(test-equal "whose entries are pairs" #t
  (let ((vars (get-environment-variables)))
    (or (null? vars) (pair? (car vars)))))

(test-equal "keyed by strings" #t
  (let ((vars (get-environment-variables)))
    (or (null? vars) (string? (car (car vars))))))

(test-equal "with string values" #t
  (let ((vars (get-environment-variables)))
    (or (null? vars) (string? (cdr (car vars))))))

;; ── exit and emergency-exit ─────────────────────────────────────────────────
;;
;; Only that they exist. Calling either ends the process, which a test in a
;; shared interpreter cannot do — and under this suite would take every later
;; file with it.

(test-equal "exit is a procedure" #t (procedure? exit))
(test-equal "emergency-exit is a procedure" #t (procedure? emergency-exit))

(test-end)
