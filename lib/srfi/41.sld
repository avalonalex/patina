; From https://github.com/scheme-requests-for-implementation/srfi-41
; Bundled by Patina from snow-fort's srfi-41 0.1.0 (Retropikzel's R7RS port,
; MIT), byte-identical apart from the two lines below that export and include
; `stream-match`; see `41-match.scm` and `PROVENANCE.md`.

(define-library
  (srfi 41)
  (import (scheme base))
  (export stream-null
          stream-cons
          stream?
          stream-null?
          stream-pair?
          stream-car
          stream-cdr
          stream-lambda
          define-stream
          list->stream
          port->stream
          stream
          stream->list
          stream-append
          stream-concat
          stream-constant
          stream-drop
          stream-drop-while
          stream-filter
          stream-fold
          stream-for-each
          stream-from
          stream-iterate
          stream-length
          stream-let
          stream-map
          stream-match
          _
          stream-of
          stream-range
          stream-ref
          stream-reverse
          stream-scan
          stream-take
          stream-take-while
          stream-unfold
          stream-unfolds
          stream-zip)
  (cond-expand
    ((or cyclone mit-scheme)
     (export make-stream-pare
             make-stream
             stream-eager))
    (stklos
     (export make-stream-pare
             make-stream
             stream-eager
             stream-delay
             stream-lazy))
    (else))
  (include "41.scm")
  ;; PATINA DEVIATION: `stream-match` in its own file — see its header. The
  ;; rest of this library is Retropikzel's R7RS port of the SRFI's reference
  ;; implementation, byte-identical, which comments `stream-match` out
  ;; because the reference writes it in `syntax-case`. `_` is exported with
  ;; it: it is the wildcard the pattern macros match as a literal, so an
  ;; importer's own `_` must be the same identifier.
  (include "41-match.scm"))
