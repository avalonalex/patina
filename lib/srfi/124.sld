;; SRFI 124: ephemerons.
;;
;; Patina-authored, over primitives in `(patina internal ephemeron)`. There is
;; no Scheme implementation to bundle: an ephemeron's defining property is that
;; its datum is kept alive only while its *key* is reachable by some other
;; path, which no Scheme-level data structure can express — it is a statement
;; about what the collector does. The behaviour lives in `heap::gc`'s ephemeron
;; fixpoint; this library is only the SRFI's surface over it.

(define-library (srfi 124)
  (import (patina internal ephemeron))
  (export make-ephemeron ephemeron? ephemeron-broken?
          ephemeron-key ephemeron-datum reference-barrier))
