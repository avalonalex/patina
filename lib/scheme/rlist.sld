;; (scheme rlist) - R7RS-large Red Edition
;;
;; Random-access lists.
;;
;; R7RS-large names this library `(scheme rlist)`; it is SRFI 101 under its
;; standard-track name -- but *not* under its names. SRFI 101 deliberately
;; shadows the list operations it replaces, exporting `cons`, `car`, `list?`
;; and so on, which is why a program using it imports `(scheme base)` with
;; those excluded. R7RS-large wants both libraries importable together, so
;; every name gains an `r`: `rcons`, `rcar`, `rlist?`.
;;
;; Three do not simply gain a prefix, because the plain prefix would read
;; wrongly: `make-list` is `make-rlist`, and the two conversions are
;; `rlist->list` and `list->rlist`.

(define-library (scheme rlist)
  (import (srfi 101))
  (export
    (rename quote rquote) (rename pair? rpair?) (rename cons rcons)
    (rename car rcar) (rename cdr rcdr) (rename caar rcaar)
    (rename cadr rcadr) (rename cddr rcddr) (rename cdar rcdar)
    (rename caaar rcaaar) (rename caadr rcaadr) (rename caddr rcaddr)
    (rename cadar rcadar) (rename cdaar rcdaar) (rename cdadr rcdadr)
    (rename cdddr rcdddr) (rename cddar rcddar) (rename caaaar rcaaaar)
    (rename caaadr rcaaadr) (rename caaddr rcaaddr) (rename caadar rcaadar)
    (rename cadaar rcadaar) (rename cadadr rcadadr) (rename cadddr rcadddr)
    (rename caddar rcaddar) (rename cdaaar rcdaaar) (rename cdaadr rcdaadr)
    (rename cdaddr rcdaddr) (rename cdadar rcdadar) (rename cddaar rcddaar)
    (rename cddadr rcddadr) (rename cddddr rcddddr) (rename cdddar rcdddar)
    (rename null? rnull?) (rename list? rlist?) (rename list rlist)
    (rename make-list make-rlist) (rename length rlength)
    (rename append rappend) (rename reverse rreverse)
    (rename list-tail rlist-tail) (rename list-ref rlist-ref)
    (rename list-set rlist-set) (rename list-ref/update rlist-ref/update)
    (rename map rmap) (rename for-each rfor-each)
    (rename random-access-list->linear-access-list rlist->list)
    (rename linear-access-list->random-access-list list->rlist)))
