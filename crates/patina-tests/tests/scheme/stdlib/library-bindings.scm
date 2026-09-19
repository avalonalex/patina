;; An import is a binding, not a snapshot of one.
;;
;; R7RS §5.2 has an import set name "a set of *bindings* from a library". So
;; the importer and the library share a location: what the library assigns
;; afterwards, the importer sees. Patina used to install the *value* the
;; variable held at import time (#406, recorded 2026-08-19), so every row here
;; but the last read a stale copy — `0` where the library had counted to 2.
;;
;; chibi's R7RS suite is 1226 of 1226 across that defect, because nothing in
;; it assigns a variable another library imported. That is the whole argument
;; for rows like these: the defect was not exotic, only outside what one suite
;; happens to do.
;;
;; ── Why this file needs libraries, and what that costs ─────────────────────
;;
;; The subject is what crosses a library boundary, so the libraries are defined
;; inline. **chibi 0.12 cannot run this file** — it does not take
;; `define-library` in a script; `expansion/template-references.scm` has the
;; measurement — and is registered `*` / `incomplete` in `DIVERGENCES.tsv`.
;; Gauche runs it. The same programs were measured against chibi with the
;; libraries as files (2026-09-19), and chibi and Gauche agree on every row.
;;
;; Each row resets the counter it reads, so rows do not depend on their order.

(import (scheme base) (scheme eval) (srfi 64))

(define-library (lb counter)
  (export count bump peek set-count! (rename count tally)
          zero-count! greet switch-greeting!)
  (import (scheme base))
  (begin
    (define count 0)
    (define (bump) (set! count (+ count 1)))
    (define (set-count! v) (set! count v))
    (define (peek) count)
    ;; Assigns from the importer's side of the boundary: the expansion runs
    ;; where the macro is used, and reaches `count` where it was defined.
    (define-syntax zero-count!
      (syntax-rules () ((_) (set! count 0))))
    (define (greet) 'hello)
    (define (switch-greeting!) (set! greet (lambda () 'goodbye)))))

;; Exports what it imported, once under the same name and once renamed.
(define-library (lb relay)
  (export (rename count relayed) (rename count relayed-again))
  (import (scheme base) (lb counter)))

;; Re-exports a procedure of `(scheme base)`, uses it, and assigns it. In
;; Patina that procedure is a primitive registered from Rust, which a first
;; version of #406 went on copying — so whether an importer saw this library's
;; assignment depended on whether it imported before or after it.
(define-library (lb primitive)
  (export exact-integer-sqrt library-isqrt replace-isqrt!)
  (import (scheme base))
  (begin
    (define (library-isqrt n)
      (call-with-values (lambda () (exact-integer-sqrt n)) list))
    (define (replace-isqrt! f) (set! exact-integer-sqrt f))))

;; A second counter, so the row that rebinds a name does not disturb the rest.
(define-library (lb shadowed)
  (export total add! read-total)
  (import (scheme base))
  (begin
    (define total 0)
    (define (add! n) (set! total (+ total n)))
    (define (read-total) total)))

;; Every kind of import set, because each is resolved by its own code: a
;; plain one installs from the library, and the other four pass the bindings
;; through a scratch environment first, where a binding could be flattened
;; into a value without any row above noticing.
(import (lb counter)
        (prefix (lb counter) c:)
        (only (lb counter) count)
        (rename (only (lb counter) count) (count renamed-count))
        (prefix (except (lb counter) bump peek set-count! tally
                        zero-count! greet switch-greeting!)
                except:)
        (lb relay)
        (lb primitive)
        (lb shadowed))

(test-begin "library-bindings")

;; ─── The importer sees what the library assigns ─────────────────────────────

(test-equal "an imported variable is the library's binding, not a copy of it"
  '(2 2)
  (begin (set-count! 0) (bump) (bump)
         (list count (peek))))

(test-equal "and so is one exported under another name" 3
  (begin (set-count! 0) (bump) (bump) (bump)
         tally))

(test-equal "and one imported under a prefix" 4
  (begin (set-count! 0) (bump) (bump) (bump) (bump)
         c:count))

;; The reference is compiled before the assignment happens, which is the
;; shape a cached global lookup would get wrong.
(define (read-count) count)
(test-equal "a procedure closed over the import reads the current value"
  '(0 1 2)
  (begin (set-count! 0)
         (let* ((a (read-count)) (b (begin (bump) (read-count))) (c (begin (bump) (read-count))))
           (list a b c))))

;; `only`, `rename` and `except`, each stacked on another set.
(test-equal "an import set that filters or renames carries the binding through"
  '(6 6)
  (begin (set-count! 0) (bump) (bump) (bump) (bump) (bump) (bump)
         (list renamed-count except:count)))

;; The macro's `set!` was always routed to the library's location. What was
;; wrong is that the importer's `count` was somewhere else.
(test-equal "an exported macro's assignment is one the importer sees" '(0 0)
  (begin (set-count! 8)
         (zero-count!)
         (list count (peek))))

;; A procedure is a variable like any other, and replacing one is how a
;; library swaps an implementation in. `call-greet` is compiled while `greet`
;; is still the first procedure.
(define (call-greet) (greet))
(test-equal "a procedure the library replaces is replaced for its importers"
  '(hello goodbye goodbye)
  (let ((before (call-greet)))
    (switch-greeting!)
    (list before (call-greet) (c:greet))))

;; `environment` imports too, by its own route.
(test-equal "an environment holds the library's binding, as an import does"
  '(2 2)
  (begin (set-count! 0) (bump)
         (let ((e (environment '(scheme base) '(lb counter))))
           (bump)
           (list (eval 'count e) (eval '(peek) e)))))

;; A re-export is the same binding again, however many libraries it passes
;; through and whatever they call it.
(test-equal "a re-exported variable is still the first library's binding"
  '(5 5)
  (begin (set-count! 0) (bump) (bump) (bump) (bump) (bump)
         (list relayed relayed-again)))

;; One location holds one object.
(test-equal "the importer and the library hold the same object" #t
  (begin (set-count! (list 1 2 3))
         (eq? count (peek))))

;; ─── What the importer does to the name ─────────────────────────────────────

;; R7RS §5.2 makes assigning an imported binding "an error", so any answer
;; conforms. chibi and Gauche both treat it as the one location it is: the
;; library sees the importer's assignment. Patina used to assign its copy and
;; leave the library's alone.
(test-equal "an importer's assignment reaches the library" '(10 10)
  (begin (set-count! 0)
         (set! count 10)
         (list count (peek))))

;; The same through `eval`: the environment's `count` is the library's, so
;; an assignment evaluated there is one everybody sees. Patina answered
;; `(0 1 5)` — three names, three locations.
(test-equal "an assignment evaluated in an environment reaches the library"
  '(5 5 5)
  (begin (set-count! 1)
         (let ((e (environment '(scheme base) '(lb counter))))
           (eval '(set! count 5) e)
           (list count (peek) (eval 'count e)))))

;; ─── A procedure of (scheme base) is a binding like any other ───────────────
;;
;; "An error" again, and again chibi and Gauche agree: `exact-integer-sqrt` is
;; one location, whoever imported it and whoever assigns it. They stop agreeing
;; only for what each inlines at compile time — `car` is an opcode in chibi,
;; Gauche inlines `length` — which is an optimization showing through, not a
;; rule to mirror, and Patina's VM deoptimizes its own inlined primitive calls
;; on exactly this event so that it never shows. Each row puts the procedure
;; back, since the location is everybody's.

(test-equal "a program's assignment to a base procedure reaches a library"
  '((replaced) (replaced) (4 0))
  (let ((original exact-integer-sqrt))
    (set! exact-integer-sqrt (lambda (n) 'replaced))
    (let ((seen (list (list (exact-integer-sqrt 16)) (library-isqrt 16))))
      (set! exact-integer-sqrt original)
      (append seen (list (library-isqrt 16))))))

(test-equal "and a library's assignment to one reaches the program"
  '((replaced) (replaced) (4 0))
  (let ((original exact-integer-sqrt))
    (replace-isqrt! (lambda (n) 'replaced))
    (let ((seen (list (list (exact-integer-sqrt 16)) (library-isqrt 16))))
      (replace-isqrt! original)
      (append seen (list (library-isqrt 16))))))

;; A definition is a new binding in the importer, and the library's is
;; untouched by it. This row was right before #406 too; it is here so that
;; sharing the location does not reach further than it should.
(define total 99)
(test-equal "an importer's definition is its own, and does not reach the library"
  '(99 7)
  (begin (add! 7)
         (list total (read-total))))

(test-end)
