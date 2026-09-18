;; (scheme regex) - R7RS-large Tangerine Edition
;;
;; Regular expressions, written as SREs -- s-expressions rather than a string
;; syntax, so a pattern is data the reader already understands.
;;
;; R7RS-large names this library `(scheme regex)`; it is SRFI 115 under its
;; standard-track name. This is a pure re-export of `(srfi 115)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme regex)
  (import (srfi 115))
  (export
    regexp regexp? valid-sre? rx regexp->sre char-set->sre regexp-matches
    regexp-matches? regexp-search regexp-replace regexp-replace-all
    regexp-match->list regexp-fold regexp-extract regexp-split
    regexp-partition regexp-match? regexp-match-count
    regexp-match-submatch regexp-match-submatch-start
    regexp-match-submatch-end))
