;;; (rnrs base) — the R6RS name for (r6rs base).
;;;
;;; A re-export and nothing else. The implementation is William D Clinger's
;;; R7RS port, bundled byte-identical under `lib/r6rs/`; this file exists so
;;; that R6RS source, which imports `(rnrs ...)`, resolves against it without
;;; being rewritten. See `lib/r6rs/PROVENANCE.md`.
;;;
;;; The export list is mechanically the one in `lib/r6rs/base.sld`, with the
;;; external half of any `rename`. R7RS has no re-export-everything form, so it
;;; is written out; a name added upstream has to be added here too, which
;;; `crates/patina-tests/tests/r6rs_rnrs_shims.rs` checks.

(define-library (rnrs base)
  (import (r6rs base))
  (export
    begin if lambda quote set! and or define define-syntax let-syntax
    letrec-syntax _ ... let let* letrec letrec* let-values let*-values case
    cond else => quasiquote unquote unquote-splicing syntax-rules assert
    identifier-syntax * + - / < <= = > >= abs append apply boolean?
    call-with-current-continuation call-with-values car cdr caar cadr cdar
    cddr ceiling char? char->integer char=? char<? char>? char<=? char>=?
    complex? cons denominator dynamic-wind eq? equal? eqv? even? exact? expt
    floor for-each gcd inexact? integer->char integer? lcm length list
    list->string list->vector list-ref list-tail list? make-string
    make-vector map max min negative? not null? number->string number?
    numerator odd? pair? positive? procedure? rational? rationalize real?
    reverse round string string->list string->number string->symbol
    string-append string-copy string-length string-ref string<=? string<?
    string=? string>=? string>? string? substring symbol->string symbol?
    truncate values vector vector->list vector-fill! vector-length
    vector-ref vector-set! vector? zero? caaar caadr cadar caddr cdaar cdadr
    cddar cdddr caaaar caaadr caadar caaddr cadaar cadadr caddar cadddr
    cdaaar cdaadr cdadar cdaddr cddaar cddadr cdddar cddddr acos asin atan
    cos exp log sin sqrt tan angle imag-part magnitude make-polar
    make-rectangular real-part boolean=? call/cc error exact
    exact-integer-sqrt inexact symbol=? string-for-each vector-map
    vector-for-each finite? infinite? nan? real-valued? rational-valued?
    integer-valued? div mod div-and-mod div0 mod0 div0-and-mod0
    assertion-violation))
