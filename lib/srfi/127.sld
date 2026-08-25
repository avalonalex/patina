;; SRFI 127: lazy sequences.
;;
;; `127/lseqs-impl.scm` is the SRFI's own reference implementation,
;; byte-identical (John Cowan, MIT); see `PROVENANCE.md`. Only this library
;; declaration is local: upstream's names the library `(lseqs)`, and its
;; generator procedures come from `(srfi 158)`, which Patina bundles, rather
;; than from the `(srfi 121)` of the SRFI's day.
(define-library (srfi 127)
  (import (scheme base) (scheme case-lambda) (srfi 158))
  (export generator->lseq lseq? lseq=?
          lseq-car lseq-first lseq-cdr lseq-rest lseq-ref lseq-take lseq-drop
          lseq-realize lseq->generator lseq-length lseq-append lseq-zip
          lseq-map lseq-for-each lseq-filter lseq-remove
          lseq-find lseq-find-tail lseq-take-while lseq-drop-while
          lseq-any lseq-every lseq-index lseq-member lseq-memq lseq-memv)
  (include "127/lseqs-impl.scm"))
