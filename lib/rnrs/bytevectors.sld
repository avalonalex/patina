;;; (rnrs bytevectors) — the R6RS name for (r6rs bytevectors).
;;;
;;; A re-export and nothing else. The implementation is William D Clinger's
;;; R7RS port, bundled byte-identical under `lib/r6rs/`; this file exists so
;;; that R6RS source, which imports `(rnrs ...)`, resolves against it without
;;; being rewritten. See `lib/r6rs/PROVENANCE.md`.
;;;
;;; The export list is mechanically the one in `lib/r6rs/bytevectors.sld`, with the
;;; external half of any `rename`. R7RS has no re-export-everything form, so it
;;; is written out; a name added upstream has to be added here too, which
;;; `crates/patina-tests/tests/r6rs_rnrs_shims.rs` checks.

(define-library (rnrs bytevectors)
  (import (r6rs bytevectors))
  (export
    endianness native-endianness bytevector? make-bytevector
    bytevector-length bytevector=? bytevector-fill! bytevector-copy!
    bytevector-copy bytevector-u8-ref bytevector-s8-ref bytevector-u8-set!
    bytevector-s8-set! bytevector->u8-list u8-list->bytevector
    bytevector-uint-ref bytevector-sint-ref bytevector-uint-set!
    bytevector-sint-set! bytevector->uint-list bytevector->sint-list
    uint-list->bytevector sint-list->bytevector bytevector-u16-ref
    bytevector-s16-ref bytevector-u16-native-ref bytevector-s16-native-ref
    bytevector-u16-set! bytevector-s16-set! bytevector-u16-native-set!
    bytevector-s16-native-set! bytevector-u32-ref bytevector-s32-ref
    bytevector-u32-native-ref bytevector-s32-native-ref bytevector-u32-set!
    bytevector-s32-set! bytevector-u32-native-set!
    bytevector-s32-native-set! bytevector-u64-ref bytevector-s64-ref
    bytevector-u64-native-ref bytevector-s64-native-ref bytevector-u64-set!
    bytevector-s64-set! bytevector-u64-native-set!
    bytevector-s64-native-set! bytevector-ieee-single-native-ref
    bytevector-ieee-single-ref bytevector-ieee-double-native-ref
    bytevector-ieee-double-ref bytevector-ieee-single-native-set!
    bytevector-ieee-single-set! bytevector-ieee-double-native-set!
    bytevector-ieee-double-set! string->utf8 string->utf16 string->utf32
    utf8->string utf16->string utf32->string))
