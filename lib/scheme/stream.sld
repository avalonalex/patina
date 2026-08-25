;; (scheme stream) - R7RS-large Red Edition
;;
;; Streams (lazy lists).
;;
;; R7RS-large names this library `(scheme stream)`; it is SRFI 41 under its
;; standard-track name. This is a pure re-export of `(srfi 41)` -- the
;; implementation lives there, and the two are the same bindings.

(define-library (scheme stream)
  (import (srfi 41))
  (export
    stream-null stream-cons stream? stream-null? stream-pair? stream-car
    stream-cdr stream-lambda define-stream list->stream port->stream stream
    stream->list stream-append stream-concat stream-constant stream-drop
    stream-drop-while stream-filter stream-fold stream-for-each stream-from
    stream-iterate stream-length stream-let stream-map stream-match _
    stream-of stream-range stream-ref stream-reverse stream-scan stream-take
    stream-take-while stream-unfold stream-unfolds stream-zip))
