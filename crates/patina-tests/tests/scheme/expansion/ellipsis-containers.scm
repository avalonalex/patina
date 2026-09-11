;; Ellipsis handling across every container shape a syntax-rules form can take.
;;
;; Migrated from `crates/patina-tests/tests/ellipsis_containers.rs` (#193),
;; which is deleted. A sibling of `ellipsis.scm`, not a section of it: that
;; file is about *which token* is the ellipsis, and this one is about *where*
;; an ellipsis may stand.
;;
;; R7RS allows `p ...` inside proper lists, dotted lists and vectors, in both
;; patterns and templates. Patina implemented it only for proper lists: five
;; separate code paths each walked their sub-forms with a hand-rolled loop that
;; never looked for an ellipsis.
;;
;; The failures came in two flavours, and the second is why these rows assert
;; on *output* rather than merely that a macro compiles:
;;
;;   * Loud — the compiler rejected the form outright:
;;     "Pattern variable x at level 1 used at level 0", or
;;     "Ellipsis in template contains no pattern variables".
;;   * Silent — the expander built the wrong structure. `(x ... . t)` produced
;;     `((x1 x2) . t)` instead of `(x1 x2 . t)`, and `#(x ...)` would have
;;     produced `#((x1 x2))` instead of `#(x1 x2)`.
;;
;; Every container kind now routes through one shared ellipsis-aware helper per
;; phase, so a sixth container shape cannot pick up the same bug by omission.
;;
;; Found by importing `(chibi optional)`, whose `let*-to-let` helper uses the
;; dotted-tail template shape; it in turn blocks `(chibi diff)` and
;; `(chibi test)`.
;;
;; The Rust file had a separate test that the vector template *splices* rather
;; than nests. It is the vector-template row below: `equal?` to `#(1 2 3)`
;; already rules out `#((1 2 3))`.

(import (scheme base) (srfi 64))

(test-begin "ellipsis-containers")

;; ─── Dotted templates ────────────────────────────────────────────────────────

(define-syntax dotted-tail
  (syntax-rules () ((_ (x ...) t) '(x ... . t))))

(test-equal "an ellipsis before a dotted tail splices" '(1 2 3 . 9)
  (dotted-tail (1 2 3) 9))

;; With no repetitions, `(x ... . t)` is just `t` — not `(() . t)`.
(test-equal "zero repetitions collapse to the tail" 'tail
  (dotted-tail () tail))

(test-equal "a dotted subtemplate under an ellipsis, with a dotted tail"
  '((1 . 2) (3 . 4) . end)
  (let-syntax ((g (syntax-rules () ((_ ((a b) ...) t) '((a . b) ... . t)))))
    (g ((1 2) (3 4)) end)))

(test-equal "a fixed prefix before the ellipsis and the dotted tail"
  '(head 1 2 . tail)
  (let-syntax ((w (syntax-rules () ((_ a (x ...) t) '(a x ... . t)))))
    (w head (1 2) tail)))

;; Guards against the fix breaking the non-dotted path it now shares.
(test-equal "a proper-list ellipsis still works" '(1 2 3)
  (let-syntax ((p (syntax-rules () ((_ (x ...)) '(x ...)))))
    (p (1 2 3))))

;; The shape from `(chibi optional)` that exposed the bug, verbatim. The Rust
;; test only asked that it compile; the row below also runs it, through a
;; stand-in for chibi's `let*-optionals`. Each step of the first rule renames
;; its `tmp` afresh, so the two bindings are distinct — an expander that
;; reused one `tmp` would answer (2 2).
(define-syntax let*-to-let
  (syntax-rules ()
    ((let*-to-let letstar ls (vars ...) ((v . d) . rest) . body)
     (let*-to-let letstar ls (vars ... (v tmp . d)) rest . body))
    ((let*-to-let letstar ls ((var tmp . d) ...) rest . body)
     (letstar ls ((tmp . d) ... . rest)
       (let ((var tmp) ...) . body)))))

(define-syntax bind-in-order
  (syntax-rules () ((_ ls ((tmp . d) ...) body) (let* ((tmp . d) ...) body))))

(test-equal "the (chibi optional) let*-to-let shape expands and runs" '(1 2)
  (let*-to-let bind-in-order ignored () ((a 1) (b 2)) (list a b)))

;; ─── Vectors: templates ──────────────────────────────────────────────────────

(define-syntax vector-of
  (syntax-rules () ((_ (x ...)) '#(x ...))))

;; Was rejected outright at compile time. Must be #(1 2 3), never #((1 2 3)).
(test-equal "an ellipsis in a vector template splices" #(1 2 3)
  (vector-of (1 2 3)))
(test-equal "zero repetitions in a vector template" #() (vector-of ()))

;; ─── Vectors: patterns ───────────────────────────────────────────────────────

(test-equal "an ellipsis in a vector pattern" '(1 2 3)
  (let-syntax ((vp (syntax-rules () ((_ #(x ...)) '(x ...)))))
    (vp #(1 2 3))))

;; Exercises the count of elements after the ellipsis: the trailing `z` must
;; not be swallowed.
(test-equal "fixed elements around an ellipsis in a vector pattern"
  '(1 (2 3) 4)
  (let-syntax ((vt (syntax-rules () ((_ #(a x ... z)) '(a (x ...) z)))))
    (vt #(1 2 3 4))))

;; The no-ellipsis path keeps its up-front length check.
(test-equal "a fixed-length vector pattern" '(1 2)
  (let-syntax ((vf (syntax-rules () ((_ #(a b)) '(a b)))))
    (vf #(1 2))))

;; ─── Rule-level dotted pattern ───────────────────────────────────────────────

;; `(kw x ... . r)` is a valid R7RS rule pattern; the rule-pattern entry point
;; bypassed the ellipsis-aware compiler its proper-list sibling used.
(define-syntax split-tail
  (syntax-rules () ((_ x ... . r) '((x ...) r))))

(test-equal "a rule-level dotted pattern with an ellipsis" '((1 2) ())
  (split-tail 1 2))

;; The Rust original used a dotted *use*, `(split-tail 1 2 . 3)`, so the tail
;; bound to a non-list. R7RS's grammar has no such use — a macro use is
;; `(<keyword> <datum>*)`, §7.1.3 — and Gauche 0.9.15 refuses it while
;; compiling ("proper list required for function application or macro use"),
;; which would take the whole file down. So the row sits in a clause Gauche
;; does not select, for the reasons `ellipsis.scm`'s header gives at length.
;; chibi accepts it and agrees.
(cond-expand
  (gauche)
  (else
   (test-equal "a dotted macro use binds the tail to the non-list" '((1 2) 3)
     (split-tail 1 2 . 3))))

(test-end)
