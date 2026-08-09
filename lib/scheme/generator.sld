;; (scheme generator) - R7RS-large Tangerine Edition
;;
;; Generators and accumulators.
;;
;; R7RS-large names this library `(scheme generator)`; it is SRFI 158 under its
;; standard-track name. This is a pure re-export of `(srfi 158)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme generator)
  (import (srfi 158))
  (export
    generator circular-generator make-iota-generator make-range-generator
    make-coroutine-generator list->generator vector->generator reverse-vector->generator
    string->generator bytevector->generator make-for-each-generator make-unfold-generator
    gcons* gappend gcombine gfilter gremove gtake gdrop gtake-while gdrop-while gflatten
    ggroup gmerge gmap gstate-filter gdelete gdelete-neighbor-dups gindex gselect
    generator->list generator->reverse-list generator->vector generator->vector!
    generator->string generator-fold generator-map->list generator-for-each generator-find
    generator-count generator-any generator-every generator-unfold make-accumulator
    count-accumulator list-accumulator reverse-list-accumulator vector-accumulator
    reverse-vector-accumulator vector-accumulator! string-accumulator bytevector-accumulator
    bytevector-accumulator! sum-accumulator product-accumulator))
