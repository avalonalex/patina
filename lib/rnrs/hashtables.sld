;;; (rnrs hashtables) — the R6RS name for (r6rs hashtables).
;;;
;;; A re-export and nothing else. The implementation is William D Clinger's
;;; R7RS port, bundled byte-identical under `lib/r6rs/`; this file exists so
;;; that R6RS source, which imports `(rnrs ...)`, resolves against it without
;;; being rewritten. See `lib/r6rs/PROVENANCE.md`.
;;;
;;; The export list is mechanically the one in `lib/r6rs/hashtables.sld`, with the
;;; external half of any `rename`. R7RS has no re-export-everything form, so it
;;; is written out; a name added upstream has to be added here too, which
;;; `crates/patina-tests/tests/r6rs_rnrs_shims.rs` checks.

(define-library (rnrs hashtables)
  (import (r6rs hashtables))
  (export
    make-eq-hashtable make-eqv-hashtable make-hashtable hashtable?
    hashtable-size hashtable-ref hashtable-set! hashtable-delete!
    hashtable-contains? hashtable-update! hashtable-copy hashtable-clear!
    hashtable-keys hashtable-entries hashtable-equivalence-function
    hashtable-hash-function hashtable-mutable? equal-hash string-hash
    string-ci-hash symbol-hash))
