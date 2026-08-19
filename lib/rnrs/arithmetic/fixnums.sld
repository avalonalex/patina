;;; (rnrs arithmetic fixnums) — the R6RS name for (r6rs arithmetic fixnums).
;;;
;;; A re-export and nothing else. The implementation is William D Clinger's
;;; R7RS port, bundled byte-identical under `lib/r6rs/`; this file exists so
;;; that R6RS source, which imports `(rnrs ...)`, resolves against it without
;;; being rewritten. See `lib/r6rs/PROVENANCE.md`.
;;;
;;; The export list is mechanically the one in `lib/r6rs/arithmetic/fixnums.sld`, with the
;;; external half of any `rename`. R7RS has no re-export-everything form, so it
;;; is written out; a name added upstream has to be added here too, which
;;; `crates/patina-tests/tests/r6rs_rnrs_shims.rs` checks.

(define-library (rnrs arithmetic fixnums)
  (import (r6rs arithmetic fixnums))
  (export
    fixnum? fixnum-width least-fixnum greatest-fixnum fx=? fx>? fx<? fx>=?
    fx<=? fxzero? fxpositive? fxnegative? fxodd? fxeven? fxmax fxmin fx+ fx*
    fx- fxdiv-and-mod fxdiv fxmod fxdiv0-and-mod0 fxdiv0 fxmod0 fx+/carry
    fx-/carry fx*/carry fxnot fxand fxior fxxor fxif fxbit-count fxlength
    fxfirst-bit-set fxbit-set? fxcopy-bit fxbit-field fxcopy-bit-field
    fxarithmetic-shift fxarithmetic-shift-left fxarithmetic-shift-right
    fxrotate-bit-field fxreverse-bit-field))
