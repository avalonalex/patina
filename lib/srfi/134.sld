;; SRFI 134: immutable deques.
;;
;; `134/ideque-impl.scm` is the body of the SRFI's own reference
;; implementation, extracted unmodified from upstream's `srfi/134.sld` (Shiro
;; Kawai and Wolfgang Corcoran-Mathe, MIT); see `PROVENANCE.md`. Upstream ships
;; the implementation inline in its library declaration rather than as a
;; separate include, so the split into two files is ours and the code between
;; them is not.
;;
;; The import and export lists are upstream's unchanged, `(srfi 158)` included:
;; the SRFI names `(srfi 158)` for `generator->list`, and Patina bundles it.
(define-library (srfi 134)
  (import (scheme base)
          (scheme case-lambda)
          (srfi 1)
          (srfi 41)
          (only (srfi 158) generator->list))
  (export ideque ideque-tabulate ideque-unfold ideque-unfold-right
          ideque? ideque-empty? ideque= ideque-any ideque-every

          ideque-front ideque-add-front ideque-remove-front
          ideque-back  ideque-add-back  ideque-remove-back

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
          generator->ideque ideque->generator)
  (include "134/ideque-impl.scm"))
