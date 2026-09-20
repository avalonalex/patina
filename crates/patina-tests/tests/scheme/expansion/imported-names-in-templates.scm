;; A library macro's template spells something like a procedure the program
;; imported — `car`, `+`, `list`, `vector-length` — in a position that is *not*
;; a reference to it.
;;
;; These are the guard rows for #438. That fix binds a template's *reference*
;; to an imported procedure to the import's location, so that a program's later
;; definition of the spelling cannot capture it (the rows for that are in
;; `template-references.scm` and `hygiene_matrix.rs`'s `redefined` rows). Its
;; first version did it by renaming the identifier in the expansion's syntax,
;; and whether an identifier is a reference is not known there: review found
;; five silent wrong answers, every one of them a row below, and not one of
;; them caught by any lane — no suite had an import-spelled name anywhere but
;; in a call. The fix binds where a reference is emitted now, and these rows
;; are what holds it there.
;;
;; They all passed before #438, and they pass under chibi and Gauche. What
;; they say is "nothing about this shape changed".
;;
;; ── Why this file needs libraries, and what that costs ─────────────────────
;;
;; Early binding applies to a macro from *another* library, so the macros are
;; defined in libraries inline. **chibi 0.12 cannot run this file** — it does
;; not take `define-library` in a script; `expansion/template-references.scm`
;; has the measurement — and is registered `*` / `incomplete` in
;; `DIVERGENCES.tsv`. Gauche runs it. Every row was also run under chibi with
;; the libraries as files (2026-09-19), and it agrees on all but two, each of
;; which says so where it stands: the `cond-expand` row, where chibi itself
;; fails — `cond-expand: bad feature`, its own `not` having been renamed by its
;; own expander, the same mistake made upstream — and the last row, where a
;; reference comes *before* the definition its expansion introduces.

(import (scheme base) (srfi 64))

(define-library (names positions)
  (import (scheme base))
  (export arith type-case with-car first-of or2 arrow values-named counting
          wrap names-of pair-record feature-test)
  (begin
    ;; `case` data.
    (define-syntax arith
      (syntax-rules ()
        ((_ op a b) (case op ((+) (+ a b)) ((-) (- a b)) (else 'no-match)))))
    (define-syntax type-case
      (syntax-rules ()
        ((_ x) (case x ((list) 'is-list) ((vector) 'is-vector) (else 'no-match)))))
    ;; Binders: `let`, `let-values`, `do`.
    (define-syntax with-car (syntax-rules () ((_ e body) (let ((car e)) body))))
    (define-syntax first-of (syntax-rules () ((_ x) (car x))))
    (define-syntax or2 (syntax-rules () ((_ a b) (let ((list a)) (if list list b)))))
    (define-syntax values-named
      (syntax-rules () ((_ e) (let-values (((max min) e)) (list max min)))))
    (define-syntax counting
      (syntax-rules () ((_ n) (do ((length 0 (+ length 1))) ((= length n) length)))))
    ;; `cond`'s `=>`, a keyword beside references.
    (define-syntax arrow (syntax-rules () ((_ c a b) (cond (c => (lambda (v) a)) (else b)))))
    ;; Another macro's literals.
    (define-syntax classify
      (syntax-rules (list vector)
        ((_ (list . r)) 'a-list)
        ((_ (vector . r)) 'a-vector)
        ((_ other) 'other)))
    (define-syntax wrap (syntax-rules () ((_ x) (classify (list x)))))
    ;; Data, by way of a macro that quotes.
    (define-syntax quote-all (syntax-rules () ((_ x ...) '(x ...))))
    (define-syntax names-of (syntax-rules () ((_) (quote-all car cdr list))))
    ;; Field names of a record type.
    (define-syntax pair-record
      (syntax-rules ()
        ((_ a b)
         (let ()
           (define-record-type pare (kons car cdr) pare? (car kar) (cdr kdr))
           (list (kar (kons a b)) (kdr (kons a b)))))))
    ;; A feature requirement's operator.
    (define-syntax feature-test
      (syntax-rules ()
        ((_) (cond-expand ((not no-such-feature-xyz) 'absent) (else 'present)))))))

;; Templates that *define* a name spelled like an import.
(define-library (names definers)
  (import (scheme base))
  (export size-of define-length define-own-length define-own-length-after)
  (begin
    (define-syntax size-of (syntax-rules () ((_ v) (vector-length v))))
    ;; Introduces the definition and nothing else.
    (define-syntax define-length
      (syntax-rules () ((_) (define (vector-length v) 'introduced))))
    ;; Introduces it and refers to it, in one expansion — the reference is the
    ;; introduced definition's, not the import's, and at the moment it is
    ;; desugared the definition is in no environment to say so.
    (define-syntax define-own-length
      (syntax-rules ()
        ((_ getter)
         (begin (define (string-length s) 'introduced)
                (define (getter) (string-length "abc"))))))
    ;; The same with the reference first.
    (define-syntax define-own-length-after
      (syntax-rules ()
        ((_ getter)
         (begin (define (getter) (bytevector-length (bytevector 1 2 3)))
                (define (bytevector-length b) 'introduced)))))))

(import (names positions) (names definers))

(test-begin "imported-names-in-templates")

;; ─── Positions that are not references ──────────────────────────────────────

(test-equal "a case datum spelled like an imported procedure still matches"
  '(3 -1 no-match is-list is-vector)
  (list (arith '+ 1 2) (arith '- 1 2) (arith '* 1 2)
        (type-case 'list) (type-case 'vector)))

;; `with-car` binds its own `car`; `first-of`'s `car`, from another expansion,
;; is the procedure. Renamed alike, the binder captured the reference.
(test-equal "a template's binder does not capture another template's reference" 1
  (with-car 5 (first-of '(1 2))))

(test-equal "a let binder spelled like an import binds" '(7 #t)
  (list (or2 #f 7) (or2 #t 7)))

(test-equal "and a let-values binder" '(1 2)
  (values-named (values 1 2)))

(test-equal "and a do variable" 3 (counting 3))

(test-equal "a keyword beside references is untouched" '(yes no)
  (list (arrow #t 'yes 'no) (arrow #f 'yes 'no)))

(test-equal "another macro's literals match a template's spelling of them"
  'a-list
  (wrap 1))

;; The alias's spelling is not something a program should ever see.
(test-equal "names handed to a quoting macro arrive as written" '(car cdr list)
  (names-of))

(test-equal "record field names spelled like imports" '(1 2)
  (pair-record 1 2))

(test-equal "a feature requirement's operator" 'absent (feature-test))

;; ─── A template that defines the spelling ───────────────────────────────────

;; `define-length`'s definition is its own expansion's. `size-of`'s reference
;; is the procedure, before and after, and so is the program's.
(define (measure-before v) (size-of v))
(define-length)
(define (measure-after v) (size-of v))

(test-equal "a template's definition of the spelling captures nobody else's reference"
  '(2 2 2)
  (list (measure-before (vector 1 2))
        (measure-after (vector 1 2))
        (vector-length (vector 1 2))))

;; And its own expansion's reference *is* that definition. When the reference
;; is desugared the definition is in no environment yet, so it looks headed
;; for the import and is bound to it — and unbound again at the end of the
;; form, when the definition has been seen (`settle_early_bindings`). Checked
;; by mutation: with that step off, this row and the next fail and no other.
(define-own-length own-length)

(test-equal "a reference beside a definition its expansion introduced is that definition"
  '(introduced 3)
  (list (own-length) (string-length "abc")))

;; The same with the reference first. **chibi 0.12 answers `3` here** — it
;; binds the reference when it meets it, to the import, the definition not
;; having been reached — and Gauche answers `introduced`, as Patina always
;; has: an expansion's definitions are one another's, in either order. No
;; claim that chibi is wrong; the row is here so that the order cannot start
;; to matter to *us* without a row going red.
(define-own-length-after own-length-after)

(test-equal "and in either order" '(introduced 3)
  (list (own-length-after) (bytevector-length (bytevector 1 2 3))))

(test-end)
