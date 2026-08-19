;;; (rnrs unicode) — the R6RS name for (r6rs unicode).
;;;
;;; A re-export and nothing else. The implementation is William D Clinger's
;;; R7RS port, bundled byte-identical under `lib/r6rs/`; this file exists so
;;; that R6RS source, which imports `(rnrs ...)`, resolves against it without
;;; being rewritten. See `lib/r6rs/PROVENANCE.md`.
;;;
;;; The export list is mechanically the one in `lib/r6rs/unicode.sld`, with the
;;; external half of any `rename`. R7RS has no re-export-everything form, so it
;;; is written out; a name added upstream has to be added here too, which
;;; `crates/patina-tests/tests/r6rs_rnrs_shims.rs` checks.

(define-library (rnrs unicode)
  (import (r6rs unicode))
  (export
    char-upcase char-downcase char-titlecase char-foldcase char-ci=?
    char-ci<? char-ci>? char-ci<=? char-ci>=? char-general-category
    char-alphabetic? char-numeric? char-whitespace? char-upper-case?
    char-lower-case? char-title-case? string-upcase string-downcase
    string-titlecase string-foldcase string-ci=? string-ci<? string-ci>?
    string-ci<=? string-ci>=? string-normalize-nfd string-normalize-nfkd
    string-normalize-nfc string-normalize-nfkc))
