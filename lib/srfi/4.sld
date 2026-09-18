;; SRFI 4: homogeneous numeric vector datatypes.
;;
;; Patina-authored over `(r6rs bytevectors)` rather than bundled; `4.scm`'s
;; header and `lib/srfi/PROVENANCE.md` record why the SRFI's own contrib port
;; was set aside.
;;
;; Bundled for `(srfi 160)`, which is the R7RS-large `(scheme vector @)`
;; family and is built directly on these ten types.

(define-library (srfi 4)
  (import (scheme base)
          ;; The element accessors. `(scheme base)` does not have the wide or
          ;; the IEEE ones, and these are the same primitives R6RS specifies.
          (r6rs bytevectors))
  (export
    make-u8vector make-s8vector make-u16vector make-s16vector
   make-u32vector make-s32vector make-u64vector make-s64vector
   make-f32vector make-f64vector
    u8vector s8vector u16vector s16vector u32vector s32vector u64vector
   s64vector f32vector f64vector
    u8vector? s8vector? u16vector? s16vector? u32vector? s32vector?
   u64vector? s64vector? f32vector? f64vector?
    u8vector-length s8vector-length u16vector-length s16vector-length
   u32vector-length s32vector-length u64vector-length s64vector-length
   f32vector-length f64vector-length
    u8vector-ref s8vector-ref u16vector-ref s16vector-ref u32vector-ref
   s32vector-ref u64vector-ref s64vector-ref f32vector-ref f64vector-ref
    u8vector-set! s8vector-set! u16vector-set! s16vector-set!
   u32vector-set! s32vector-set! u64vector-set! s64vector-set!
   f32vector-set! f64vector-set!
    u8vector->list s8vector->list u16vector->list s16vector->list
   u32vector->list s32vector->list u64vector->list s64vector->list
   f32vector->list f64vector->list
    list->u8vector list->s8vector list->u16vector list->s16vector
   list->u32vector list->s32vector list->u64vector list->s64vector
   list->f32vector list->f64vector   )
  (include "4.scm"))
