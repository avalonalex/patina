;; (scheme show) - R7RS-large Tangerine Edition
;;
;; Combinator formatting: a formatter is a value built from combinators
;; rather than a format string, so it composes and is checked by the reader.
;;
;; R7RS-large names this library `(scheme show)`; it is SRFI 159 under its
;; standard-track name. This is a pure re-export of `(srfi 159)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme show)
  (import (srfi 159))
  (export
    call-with-output displayed each each-in-list escaped fitted
    fitted/both fitted/right fl fn forked joined joined/dot joined/last
    joined/prefix joined/range joined/suffix maybe-escaped nl nothing
    numeric numeric/comma numeric/fitted numeric/si padded padded/both
    padded/right pretty pretty-simply show space-to tab-to trimmed
    trimmed/both trimmed/lazy trimmed/right with with! written
    written-simply as-red as-blue as-green as-cyan as-yellow as-magenta
    as-white as-black as-bold as-underline call-with-output-generator
    call-with-output-generators columnar from-file justified line-numbers
    show-columns string->line-generator tabular wrapped wrapped/char
    wrapped/list as-unicode unicode-terminal-width))
