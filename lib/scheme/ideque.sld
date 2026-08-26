;; (scheme ideque) - R7RS-large Red Edition
;;
;; Immutable deques.
;;
;; R7RS-large names this library `(scheme ideque)`; it is SRFI 134 under its
;; standard-track name. This is a pure re-export of `(srfi 134)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme ideque)
  (import (srfi 134))
  (export
    ideque ideque-tabulate ideque-unfold ideque-unfold-right
    ideque? ideque-empty? ideque= ideque-any ideque-every
    ideque-front ideque-add-front ideque-remove-front
    ideque-back ideque-add-back ideque-remove-back
    ideque-ref
    ideque-take ideque-take-right ideque-drop ideque-drop-right
    ideque-split-at
    ideque-length ideque-append ideque-reverse
    ideque-count ideque-zip
    ideque-map ideque-filter-map
    ideque-for-each ideque-for-each-right
    ideque-fold ideque-fold-right
    ideque-append-map
    ideque-filter ideque-remove ideque-partition
    ideque-find ideque-find-right
    ideque-take-while ideque-take-while-right
    ideque-drop-while ideque-drop-while-right
    ideque-span ideque-break
    list->ideque ideque->list
    generator->ideque ideque->generator))
