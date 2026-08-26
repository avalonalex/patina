;; (scheme text) - R7RS-large Red Edition
;;
;; Immutable texts.
;;
;; R7RS-large names this library `(scheme text)`; it is SRFI 135 under its
;; standard-track name. This is a pure re-export of `(srfi 135)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme text)
  (import (srfi 135))
  (export
    text? textual? textual-null? textual-every textual-any make-text text
    text-tabulate text-unfold text-unfold-right textual->text textual->string
    textual->vector textual->list string->text vector->text list->text
    reverse-list->text textual->utf8 textual->utf16be textual->utf16
    textual->utf16le utf8->text utf16be->text utf16->text utf16le->text
    text-length textual-length text-ref textual-ref subtext subtextual
    textual-copy textual-take textual-take-right textual-drop
    textual-drop-right textual-pad textual-pad-right textual-trim
    textual-trim-right textual-trim-both textual-replace textual=?
    textual-ci=? textual<? textual-ci<? textual>? textual-ci>? textual<=?
    textual-ci<=? textual>=? textual-ci>=? textual-prefix-length
    textual-suffix-length textual-prefix? textual-suffix? textual-index
    textual-index-right textual-skip textual-skip-right textual-contains
    textual-contains-right textual-upcase textual-downcase textual-foldcase
    textual-titlecase textual-append textual-concatenate
    textual-concatenate-reverse textual-join textual-fold textual-fold-right
    textual-map textual-for-each textual-map-index textual-for-each-index
    textual-count textual-filter textual-remove textual-replicate
    textual-split))
