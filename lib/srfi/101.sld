;; SRFI 101: purely functional random-access pairs and lists.
;;
;; `101/rlist-impl.scm` is the body of the SRFI's own reference implementation
;; (David Van Horn, MIT), ported from R6RS; see `PROVENANCE.md`. This library
;; declaration is Patina's: upstream's is an R6RS `(library (srfi :101) …)`
;; whose export list uses R6RS's `(rename (from to))` spelling.
;;
;; Every export is renamed, because SRFI 101 deliberately shadows the names of
;; the list operations it replaces — `cons`, `car`, `list?` and the rest are
;; the random-access versions here.
;;
;; Note that importing *this* library rather than `(scheme rlist)` currently
;; breaks quoting: `(import (srfi 101)) (define x '(1 2))` recurses until the
;; stack goes, because rebinding `quote` to a macro captures the `quote` in
;; this library's own `ra:quote` template. That is triage family 33, a defect
;; in the macro expander rather than here, and it does not touch
;; `(scheme rlist)`, whose names are all `r`-prefixed.

(define-library (srfi 101)
  (import (scheme base)
          (scheme case-lambda)
          ;; `assert` and `assertion-violation` are R6RS, and the body uses
          ;; them; Patina's `(rnrs base)` shim provides both.
          (only (rnrs base) assert assertion-violation)
          (rnrs lists)
          (rnrs control)
          (rnrs hashtables)
          (only (scheme bitwise) arithmetic-shift))
  (export (rename ra:quote quote) (rename ra:pair? pair?) (rename ra:cons cons) (rename ra:car car) (rename ra:cdr cdr) (rename ra:caar caar) (rename ra:cadr cadr) (rename ra:cddr cddr) (rename ra:cdar cdar) (rename ra:caaar caaar) (rename ra:caadr caadr) (rename ra:caddr caddr) (rename ra:cadar cadar) (rename ra:cdaar cdaar) (rename ra:cdadr cdadr) (rename ra:cdddr cdddr) (rename ra:cddar cddar) (rename ra:caaaar caaaar) (rename ra:caaadr caaadr) (rename ra:caaddr caaddr) (rename ra:caadar caadar) (rename ra:cadaar cadaar) (rename ra:cadadr cadadr) (rename ra:cadddr cadddr) (rename ra:caddar caddar) (rename ra:cdaaar cdaaar) (rename ra:cdaadr cdaadr) (rename ra:cdaddr cdaddr) (rename ra:cdadar cdadar) (rename ra:cddaar cddaar) (rename ra:cddadr cddadr) (rename ra:cddddr cddddr) (rename ra:cdddar cdddar) (rename ra:null? null?) (rename ra:list? list?) (rename ra:list list) (rename ra:make-list make-list) (rename ra:length length) (rename ra:append append) (rename ra:reverse reverse) (rename ra:list-tail list-tail) (rename ra:list-ref list-ref) (rename ra:list-set list-set) (rename ra:list-ref/update list-ref/update) (rename ra:map map) (rename ra:for-each for-each) (rename ra:random-access-list->linear-access-list random-access-list->linear-access-list) (rename ra:linear-access-list->random-access-list linear-access-list->random-access-list))
  (begin
    ;; PATINA LOCAL EDIT: R6RS spells this `bitwise-arithmetic-shift`; SRFI
    ;; 151, which Patina bundles, calls the same operation `arithmetic-shift`.
    (define bitwise-arithmetic-shift arithmetic-shift))
  (include "101/rlist-impl.scm"))
