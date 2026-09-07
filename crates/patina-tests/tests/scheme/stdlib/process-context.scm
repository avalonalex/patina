;; `(scheme process-context)` — R7RS §6.14.
;;
;; Migrated from `crates/patina-tests/tests/scheme_process_context.rs` (#193
;; Phase 1). 12 rows there, 11 here — two sections asserted the same program —
;; and one added that the `.rs` file had written and commented out. **12 rows.**
;;
;; 12/12 on patina VM, patina tree-walker, chibi and Gauche.
;;
;; **These rows describe shapes, not values.** The process environment is not
;; ours to fix: `PATH`'s contents differ per machine, `command-line`'s first
;; element is explicitly implementation-dependent (R7RS §6.14), and the
;; environment block is whatever the caller exported. So each row asserts the
;; type or the structure of what comes back — never a value — and the two rows
;; naming a variable use `PATH`, which every platform this runs on defines, and
;; a name nothing defines.
;;
;; `argv` and `env` are read once, at the top level. Each call rebuilds the
;; whole result on the heap, and the driver runs every file on both backends;
;; four calls to `get-environment-variables` is four copies of the block per
;; backend for no added coverage. Top level rather than a `(let () …)` wrapper
;; deliberately — see `expansion/define-values.scm` on what that wrapper
;; changes.

(import (scheme base) (scheme process-context) (srfi 64))

(test-begin "process-context")

;; ── command-line ────────────────────────────────────────────────────────────

(define argv (command-line))

(test-equal "command-line returns a list" #t (list? argv))

;; Ordered before the row that takes its `car`: if this list were ever empty,
;; the row below raises from `car` and reports a bare comparison failure, while
;; this one names the cause.
(test-equal "command-line always has at least one element" #t (>= (length argv) 1))

;; R7RS §6.14: "the first element is an implementation-dependent string
;; identifying the calling program" — it may even be empty, so the row says it
;; is a string, not what it holds.
(test-equal "command-line's first element is a string" #t (string? (car argv)))

;; ── get-environment-variable ────────────────────────────────────────────────

;; The `.rs` file asserted this twice, in two sections, with the same program;
;; once is enough. `PATH` is the variable to use because it is the one every
;; platform defines — the row is about the return type, not the value.
(test-equal "a variable that exists reads back as a string" #t
  (string? (get-environment-variable "PATH")))

(test-equal "a variable that does not exist reads back as #f" #f
  (get-environment-variable "PATINA_NONEXISTENT_VAR_12345"))

;; A non-string name is an error, and a catchable one.
;;
;; This row existed in the `.rs` file — commented out, with the reason "Can't
;; test type error without guard, which isn't implemented yet". `guard` has been
;; implemented for a long time, so the note outlived its premise and the row
;; stayed dark. Written out and enabled here.
;;
;; The clause tests `error-object?` rather than `#t`, which is stronger but not
;; as strong as it looks. It rules out a raise of a *non-error* object, which a
;; catch-all would accept. It does **not** rule out a different error: an
;; unbound-variable error is itself an error object on both backends (measured),
;; so a typo'd or unexported `get-environment-variable` would still satisfy this
;; row on its own. What rules that out is the positive row above — if the name
;; stopped resolving, "a variable that exists reads back as a string" fails too.
;;
;; Asserting the message would discriminate, and is what this repo declines to
;; do: the backends word diagnostics differently, and chibi and Gauche
;; differently again. Portable as written — `error-object?` answers #t for this
;; raise on all four implementations.
;;
;; Its unguarded half — that the same program fails a top-level program with
;; nothing to catch it — is `callability.rs`, per the pairing that file
;; documents: routing changes *where* an error is delivered, never whether it is
;; raised.
(test-equal "a non-string name raises an error object" 'type-error
  (guard (e ((error-object? e) 'type-error))
    (get-environment-variable 123)))

;; ── get-environment-variables ───────────────────────────────────────────────

(define env (get-environment-variables))

;; An association list of strings. Each row folds over the **whole** block, not
;; just its head: a backend that built one good entry and malformed the rest
;; would satisfy a `(car env)` check, which is what the `.rs` rows did.
;;
;; Every row also admits the empty case. R7RS does not promise the block is
;; non-empty, and a row that indexed into it blindly would be asserting about
;; the machine rather than about the procedure.

(test-equal "get-environment-variables returns a list" #t (list? env))

(test-equal "every entry is a pair" #t
  (let loop ((v env)) (or (null? v) (and (pair? (car v)) (loop (cdr v))))))

(test-equal "every key is a string" #t
  (let loop ((v env)) (or (null? v) (and (string? (car (car v))) (loop (cdr v))))))

(test-equal "every value is a string" #t
  (let loop ((v env)) (or (null? v) (and (string? (cdr (car v))) (loop (cdr v))))))

;; ── exit and emergency-exit ─────────────────────────────────────────────────
;;
;; Only that they exist. Calling either ends the process, which a test in a
;; shared interpreter cannot do — and under this suite would take every later
;; file with it.

(test-equal "exit is a procedure" #t (procedure? exit))
(test-equal "emergency-exit is a procedure" #t (procedure? emergency-exit))

(test-end)
