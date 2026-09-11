;; R7RS §5.5 record types: `define-record-type`, and the primitives under it.
;;
;; Migrated from `crates/patina-tests/tests/record_types.rs` (#193 Phase 2),
;; which is deleted. That file built a `TreeWalkInterpreter` by hand for every
;; test, so **none of its 41 tests ever ran on the VM**, the default backend —
;; the same coverage gap `hygiene.rs` had. Every row here runs on both.
;;
;; Two kinds of row, kept apart:
;;
;;   - `define-record-type` as R7RS specifies it — construction, predicates,
;;     accessors, modifiers, disjointness, generativity. Portable, and
;;     answered by chibi and Gauche on the oracle lane.
;;   - Patina's own record machinery: the `%record-*` primitives the macro
;;     expands to (`lib/scheme/base/records.scm`), the record type as a value,
;;     and how a record prints. `(scheme base)` exports those primitives —
;;     they would be unreachable otherwise (`primitives_reachable_by_import`)
;;     — so a portable file can name them, but only Patina has them. Each such
;;     row is scoped with `(cond-expand (patina) (else (test-skip 1)))`, so the
;;     oracles *report* a skip rather than losing the row, and each keeps its
;;     `define`s inside its own expression so nothing unbound runs at top
;;     level elsewhere.
;;
;; Divergences are recorded in `DIVERGENCES.tsv`, not restated here.

(import (scheme base) (srfi 64))

(test-begin "record-types")

;; ── The record machinery (Patina-specific) ─────────────────────────────────
;;
;; A record type descriptor is `%make-record-type name fields`; a record is
;; `%make-record rtd field-vector`; fields are addressed by index. This is the
;; layer `define-record-type` expands to.

(cond-expand (patina) (else (test-skip 1)))
(test-equal "%make-record-type makes a record type descriptor" '(#t #f #f)
  (list (%record-type? (%make-record-type 'point '(x y)))
        (%record-type? 42)
        (%record-type? '())))

(cond-expand (patina) (else (test-skip 1)))
(test-assert "%make-record makes a record"
  (let ((rtd (%make-record-type 'point '(x y))))
    (%record? (%make-record rtd (vector 10 20)))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "%record-ref reads a field by index" '(10 20)
  (let* ((rtd (%make-record-type 'point '(x y)))
         (r (%make-record rtd (vector 10 20))))
    (list (%record-ref r 0) (%record-ref r 1))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "%record-set! writes a field by index" 100
  (let* ((rtd (%make-record-type 'point '(x y)))
         (r (%make-record rtd (vector 10 20))))
    (%record-set! r 0 100)
    (%record-ref r 0)))

(cond-expand (patina) (else (test-skip 1)))
(test-assert "%record-type-of answers the descriptor the record was made with"
  (let* ((rtd (%make-record-type 'point '(x y)))
         (r (%make-record rtd (vector 10 20))))
    (eq? (%record-type-of r) rtd)))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "a descriptor knows its name and its fields" '(point (x y z))
  (let ((rtd (%make-record-type 'point '(x y z))))
    (list (%record-type-name rtd) (%record-type-fields rtd))))

(cond-expand (patina) (else (test-skip 1)))
(test-equal "%record-type-field-index finds a field, or answers #f" '(1 #f)
  (let ((rtd (%make-record-type 'point '(x y z))))
    (list (%record-type-field-index rtd 'y)
          (%record-type-field-index rtd 'w))))

;; R7RS binds the type name to "a representation of the record type itself"
;; and says nothing more; on Patina it is the descriptor `%make-record-type`
;; returns, so the introspection primitives apply to it.
(cond-expand (patina) (else (test-skip 1)))
(test-equal "the type name of a define-record-type is its descriptor"
  '(#t <point> (x y))
  (let ()
    (define-record-type <point> (make-point x y) point? (x point-x) (y point-y))
    (list (%record-type? <point>)
          (%record-type-name <point>)
          (%record-type-fields <point>))))

;; How a record and its type print is implementation-defined; these are
;; Patina's spellings, pinned exactly. (The Rust rows checked only that the
;; text *contained* "record" and the type's name.)
(cond-expand (patina) (else (test-skip 1)))
(test-equal "a record and its type print with the type's name"
  '("#<record <point>>" "#<record-type <point>>" "#<record-type point>")
  (let ()
    (define-record-type <point> (make-point x y) point? (x point-x) (y point-y))
    (define (written x)
      (let ((port (open-output-string)))
        (write x port)
        (get-output-string port)))
    (list (written (make-point 1 2))
          (written <point>)
          (written (%make-record-type 'point '(x y))))))

;; ── define-record-type ─────────────────────────────────────────────────────
;;
;; The first type is R7RS §5.5's own example, and most rows below reuse it.

(define-record-type <pare>
  (kons x y)
  pare?
  (x kar set-kar!)
  (y kdr))

(test-equal "the report's example: construct, test, access, modify"
  '(#t #f 1 2 3)
  (list (pare? (kons 1 2))
        (pare? (cons 1 2))
        (kar (kons 1 2))
        (kdr (kons 1 2))
        (let ((k (kons 1 2)))
          (set-kar! k 3)
          (kar k))))

(define-record-type <point>
  (make-point x y)
  point?
  (x point-x)
  (y point-y))

;; §3.2: a record type is disjoint from every other type — the predicates of
;; the built-in types all answer #f to a record.
(test-equal "a record is none of the built-in types"
  '(#f #f #f #f #f #f #f #f)
  (let ((p (make-point 1 2)))
    (list (pair? p) (vector? p) (null? p) (boolean? p)
          (number? p) (string? p) (symbol? p) (procedure? p))))

;; Each `define-record-type` makes a new type, even one structurally
;; identical to an existing one.
(define-record-type <point1> (make-point1 x y) point1? (x point1-x) (y point1-y))
(define-record-type <point2> (make-point2 x y) point2? (x point2-x) (y point2-y))

(test-equal "record types are generative" '(#f #f #t #t)
  (list (point1? (make-point2 1 2))
        (point2? (make-point1 1 2))
        (point1? (make-point1 1 2))
        (point2? (make-point2 1 2))))

(define-record-type <point3d>
  (make-point3d x y z)
  point3d?
  (x point3d-x)
  (y point3d-y)
  (z point3d-z set-point3d-z!))

(test-equal "a record with three fields" '(1 2 3)
  (let ((p (make-point3d 1 2 3)))
    (list (point3d-x p) (point3d-y p) (point3d-z p))))

(define-record-type <wrapper>
  (wrap value)
  wrapper?
  (value unwrap set-unwrap!))

(test-equal "a record with a single field" 42
  (unwrap (wrap 42)))

(define-record-type <box>
  (make-box contents)
  box?
  (contents unbox set-box!))

(test-equal "a field holds any value" '((a b c) #t 7)
  (list (unbox (make-box '(a b c)))
        (procedure? (unbox (make-box (lambda (x) x))))
        ((unbox (make-box (lambda (x) (+ x 1)))) 6)))

(define-record-type <rect>
  (make-rect top-left bottom-right)
  rect?
  (top-left rect-top-left)
  (bottom-right rect-bottom-right))

(test-equal "a record can hold another record" 10
  (let ((r (make-rect (make-point 0 0) (make-point 10 10))))
    (point-x (rect-bottom-right r))))

;; ── The constructor's field list ───────────────────────────────────────────

;; The constructor names fields in its own order, not the declaration's.
(define-record-type <swapped>
  (make-swapped y x)
  swapped?
  (x swapped-x)
  (y swapped-y))

(test-equal "the constructor's field order is its own" '(10 20)
  (let ((p (make-swapped 20 10)))
    (list (swapped-x p) (swapped-y p))))

;; A field the constructor does not name starts with an unspecified value;
;; a modifier can set it.
(define-record-type <partial>
  (make-partial x y)
  partial?
  (x partial-x)
  (y partial-y)
  (z partial-z set-partial-z!))

(test-equal "the constructor may name a subset of the fields" '(1 2 3)
  (let ((p (make-partial 1 2)))
    (set-partial-z! p 3)
    (list (partial-x p) (partial-y p) (partial-z p))))

;; ── Identity and equality ──────────────────────────────────────────────────

(test-equal "a record is eq? and eqv? to itself and to no other" '(#t #f #t #f)
  (let ((p (make-point 1 2)))
    (list (eq? p p)
          (eq? (make-point 1 2) (make-point 1 2))
          (eqv? p p)
          (eqv? (make-point 1 2) (make-point 1 2)))))

;; R7RS §6.1 leaves `equal?` on records open — "in all other cases, equal?
;; may return either #t or #f" — and the field splits. Patina and chibi
;; compare the fields; Gauche compares identity (registered as latitude).
;; Asserted here because it is Patina's choice and a change to it should be
;; deliberate, not because the report requires it.
(test-equal "equal? compares two records of one type by their fields" '(#t #f)
  (list (equal? (make-point 1 2) (make-point 1 2))
        (equal? (make-point 1 2) (make-point 1 3))))

;; Records of different types are never `equal?`, whatever their fields —
;; which every implementation agrees on, whichever way it reads the rule
;; above.
(test-equal "equal? never equates records of different types" #f
  (equal? (make-point1 1 2) (make-point2 1 2)))

;; ── Mutation ───────────────────────────────────────────────────────────────

(test-equal "a modifier changes one instance only" '(99 1)
  (let ((b1 (make-box 1))
        (b2 (make-box 1)))
    (set-box! b1 99)
    (list (unbox b1) (unbox b2))))

(test-equal "a modifier is seen through every reference" 99
  (let* ((b (make-box 1))
         (alias b))
    (set-box! alias 99)
    (unbox b)))

(test-end)
