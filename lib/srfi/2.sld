;; SRFI 2: AND-LET*, an AND with local bindings.
;;
;; Patina-authored, because the SRFI has no portable reference implementation
;; to bundle: Oleg Kiselyov's original is a low-level macro against a
;; `defmacro`-style expander, and the SRFI document carries no `syntax-rules`
;; version. The rules below are the shape every R7RS implementation has
;; converged on, written against `(scheme base)` rather than adapted from one
;; of them; `crates/patina-tests/tests/scheme/srfi/and-let.scm` checks each
;; clause form against the SRFI's own text, including the three claw shapes
;; and the empty-body cases the document spells out. The one case that cannot
;; be a row there is the rejection of a malformed claw, since `syntax-error`
;; fires before `guard` exists to catch it; `reexport_shims.rs` pins that.
;;
;; Bundled for `(srfi 146)`, whose `(nieper rbtree)` uses it — see
;; lib/srfi/PROVENANCE.md.

(define-library (srfi 2)
  (import (scheme base))
  (export and-let*)
  (begin
    (define-syntax and-let*
      (syntax-rules ()
        ;; No claws: the SRFI says the value is #t.
        ((and-let* ()) #t)
        ;; No claws, but a body: the body's value, in a fresh scope so an
        ;; internal define in it is legal.
        ((and-let* () . body) (let () . body))
        ;; A claw of three or more elements is none of the three shapes, and it
        ;; is rejected here — above every rule that could match it, since each
        ;; of them would *accept* it and mean something else:
        ;;
        ;;   (and-let* ((a 1 2)))      the bare-claw rule below reads the whole
        ;;                             claw as one expression and calls `a`
        ;;   (and-let* ((a 1 2)) body) the `(expr)` rule reads it as a test
        ;;                             claw `a` followed by bare claws 1 and 2
        ;;
        ;; Either way the diagnostic is "unbound variable: a", which names
        ;; neither the claw nor `and-let*`. One rule covers the claw wherever
        ;; it sits, including last with no body: `rest` and `body` both match
        ;; the empty tail.
        ((and-let* ((a b c . d) . rest) . body)
         (syntax-error "and-let* claw must be (var expr), (expr) or expr"
                       (a b c . d)))
        ;; A single trailing claw with no body yields the claw's own value,
        ;; rather than #t: `(and-let* ((x (f))))` is `(f)`.
        ((and-let* ((var expr))) expr)
        ((and-let* ((expr))) expr)
        ((and-let* (expr)) expr)
        ;; (var expr) binds, and the binding is visible to every later claw
        ;; and to the body.
        ((and-let* ((var expr) . rest) . body)
         (let ((var expr))
           (and var (and-let* rest . body))))
        ;; (expr) tests without binding.
        ((and-let* ((expr) . rest) . body)
         (and expr (and-let* rest . body)))
        ;; A bare expression tests without binding. The SRFI restricts this
        ;; form to a variable reference; accepting any expression is the
        ;; extension every implementation makes, and `tmp` keeps it evaluated
        ;; exactly once.
        ((and-let* (expr . rest) . body)
         (let ((tmp expr))
           (and tmp (and-let* rest . body))))))))
