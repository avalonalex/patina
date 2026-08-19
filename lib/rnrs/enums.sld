;;; (rnrs enums) — the R6RS name for (r6rs enums).
;;;
;;; A re-export and nothing else. The implementation is William D Clinger's
;;; R7RS port, bundled byte-identical under `lib/r6rs/`; this file exists so
;;; that R6RS source, which imports `(rnrs ...)`, resolves against it without
;;; being rewritten. See `lib/r6rs/PROVENANCE.md`.
;;;
;;; The export list is mechanically the one in `lib/r6rs/enums.sld`, with the
;;; external half of any `rename`. R7RS has no re-export-everything form, so it
;;; is written out; a name added upstream has to be added here too, which
;;; `crates/patina-tests/tests/r6rs_rnrs_shims.rs` checks.

(define-library (rnrs enums)
  (import (r6rs enums))
  (export
    make-enumeration enum-set-universe enum-set-indexer enum-set-constructor
    enum-set->list enum-set-member? enum-set-subset? enum-set=?
    enum-set-union enum-set-intersection enum-set-difference
    enum-set-complement enum-set-projection define-enumeration))
