;; (scheme r5rs) - R7RS R5RS Compatibility Library
;;
;; Provides all identifiers defined by R5RS, except transcript-on and
;; transcript-off. The R5RS names exact->inexact and inexact->exact are
;; provided as aliases for inexact and exact respectively.

(define-library (scheme r5rs)
  (import (scheme base)
          (scheme char)
          (scheme complex)
          (scheme cxr)
          (scheme eval)
          (scheme file)
          (scheme inexact)
          (scheme lazy)
          (scheme load)
          (scheme read)
          (scheme repl)
          (scheme write)
          (patina internal r5rs))

  (begin
    (define exact->inexact inexact)
    (define inexact->exact exact))

  (export
    ;; ── Arithmetic ──────────────────────────────────────────────────────
    * + - /
    < <= = > >=
    abs
    quotient remainder modulo
    gcd lcm
    numerator denominator
    floor ceiling truncate round
    rationalize
    expt
    max min
    number->string string->number
    ;; ── Number predicates ───────────────────────────────────────────────
    number? complex? real? rational? integer?
    exact? inexact?
    zero? positive? negative? odd? even?
    ;; ── R5RS exactness names ────────────────────────────────────────────
    exact->inexact inexact->exact
    ;; ── Booleans ────────────────────────────────────────────────────────
    not boolean?
    ;; ── Pairs and lists ─────────────────────────────────────────────────
    pair? cons car cdr
    set-car! set-cdr!
    caar cadr cdar cddr
    caaar caadr cadar caddr cdaar cdadr cddar cdddr
    ;; 4-deep cxr (from scheme cxr)
    caaaar caaadr caadar caaddr cadaar cadadr caddar cadddr
    cdaaar cdaadr cdadar cdaddr cddaar cddadr cdddar cddddr
    null? list? list
    length append reverse
    list-tail list-ref
    memq memv member
    assq assv assoc
    ;; ── Symbols ─────────────────────────────────────────────────────────
    symbol? symbol->string string->symbol
    ;; ── Characters ──────────────────────────────────────────────────────
    char?
    char=? char<? char>? char<=? char>=?
    char-ci=? char-ci<? char-ci>? char-ci<=? char-ci>=?
    char-alphabetic? char-numeric? char-whitespace?
    char-upper-case? char-lower-case?
    char->integer integer->char
    char-upcase char-downcase
    ;; ── Strings ─────────────────────────────────────────────────────────
    string? make-string string
    string-length string-ref string-set!
    string=? string<? string>? string<=? string>=?
    string-ci=? string-ci<? string-ci>? string-ci<=? string-ci>=?
    substring string-append
    string->list list->string
    string-copy string-fill!
    ;; ── Vectors ─────────────────────────────────────────────────────────
    vector? make-vector vector
    vector-length vector-ref vector-set!
    vector->list list->vector
    vector-fill!
    ;; ── Control ──────────────────────────────────────────────────────────
    procedure? apply map for-each
    call-with-current-continuation
    values call-with-values
    dynamic-wind
    ;; ── Eval ────────────────────────────────────────────────────────────
    eval
    null-environment scheme-report-environment
    ;; ── I/O ─────────────────────────────────────────────────────────────
    call-with-input-file call-with-output-file
    input-port? output-port?
    current-input-port current-output-port
    with-input-from-file with-output-to-file
    open-input-file open-output-file
    close-input-port close-output-port
    read read-char peek-char char-ready?
    eof-object?
    write display newline write-char
    ;; ── Load ────────────────────────────────────────────────────────────
    load
    ;; ── REPL ────────────────────────────────────────────────────────────
    interaction-environment
    ;; ── Lazy ────────────────────────────────────────────────────────────
    delay force
    ;; Internal: needed for delay macro expansion
    %make-delayed-promise
    ;; ── Inexact ─────────────────────────────────────────────────────────
    exp log sin cos tan asin acos atan sqrt
    ;; ── Complex ─────────────────────────────────────────────────────────
    make-rectangular make-polar real-part imag-part magnitude angle
    ;; ── Derived syntax (macros) ─────────────────────────────────────────
    and or cond case do
    let let* letrec
    ;; ── Core syntax ─────────────────────────────────────────────────────
    ;; Real bindings since syntax keywords became importable (see the note
    ;; in base.sld); a library whose only import is (scheme r5rs) has no
    ;; other way to reach `define` — srfi-78's reference implementation in
    ;; the vendored corpus is exactly that shape. Inventory matches chibi's
    ;; (scheme r5rs).
    begin define define-syntax if lambda quote quasiquote set!
    let-syntax letrec-syntax syntax-rules else => ... _
    ))
