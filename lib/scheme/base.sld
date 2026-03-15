;; (scheme base) - R7RS Base Library
;;
;; This library provides the core R7RS procedures and syntax.
;; It imports from domain-specific internal libraries and re-exports
;; what R7RS specifies for (scheme base).

(define-library (scheme base)
  ;; Import from domain-specific internal libraries (ordered by R7RS section)
  (import (patina internal predicates)   ; §6.1 Booleans, §6.3 Symbols
          (patina internal numbers)      ; §6.2 Numbers
          (patina internal lists)        ; §6.4 Pairs and lists
          (patina internal chars)        ; §6.6 Characters
          (patina internal strings)      ; §6.7 Strings
          (patina internal vectors)      ; §6.8 Vectors
          (patina internal bytevectors)  ; §6.9 Bytevectors
          (patina internal control)      ; §6.10 Control (apply, map, for-each, values, call/cc)
          (patina internal errors)       ; §6.11 Exceptions
          (patina internal io)           ; §6.13 I/O
          (patina internal records)      ; §5.5 Record types
          (patina internal params)       ; §4.2.6 Dynamic bindings
          (patina internal system))      ; features

  ;; R7RS (scheme base) exports
  (export
    ;; === Equivalence predicates (§6.1) ===
    eq? eqv? equal?

    ;; === Booleans (§6.1) ===
    boolean? boolean=?
    ;; not is defined in Scheme (base/lists.scm)

    ;; === Numbers (§6.2) ===
    ;; Type predicates
    number? complex? real? rational? integer?
    exact? inexact? exact-integer?
    ;; Comparison
    = < > <= >=
    ;; Arithmetic
    + - * /
    abs max min
    floor/ floor-quotient floor-remainder
    truncate/ truncate-quotient truncate-remainder
    quotient remainder modulo
    gcd lcm
    numerator denominator
    floor ceiling truncate round
    rationalize
    ;; Transcendental (only square and expt in base)
    square expt
    exact-integer-sqrt
    ;; Conversion
    exact inexact
    number->string string->number
    ;; Note: zero?, positive?, negative?, odd?, even? defined in Scheme

    ;; === Symbols (§6.3) ===
    symbol? symbol=?
    symbol->string string->symbol

    ;; === Pairs and lists (§6.4) ===
    pair? cons car cdr
    set-car! set-cdr!
    null? list?
    make-list list
    length append reverse
    list-tail list-ref list-set!
    memq memv member
    assq assv assoc
    list-copy
    ;; Note: caar, cadr, etc. defined in Scheme

    ;; === Characters (§6.6) ===
    char?
    char=? char<? char>? char<=? char>=?
    char->integer integer->char

    ;; === Strings (§6.7) ===
    string?
    make-string string
    string-length string-ref string-set!
    string=? string<? string>? string<=? string>=?
    substring string-append
    string->list list->string
    string-copy string-copy! string-fill!
    ;; Note: string-map, string-for-each are in (patina internal strings)

    ;; === Vectors (§6.8) ===
    vector?
    make-vector vector
    vector-length vector-ref vector-set!
    vector->list list->vector
    vector->string string->vector
    vector-copy vector-copy! vector-fill!
    vector-append

    ;; === Bytevectors (§6.9) ===
    bytevector?
    make-bytevector bytevector
    bytevector-length
    bytevector-u8-ref bytevector-u8-set!
    bytevector-copy bytevector-copy!
    bytevector-append
    utf8->string string->utf8

    ;; === Control features (§6.10) ===
    procedure?
    apply
    map for-each
    string-map string-for-each
    vector-map vector-for-each
    call-with-current-continuation call/cc
    values call-with-values
    dynamic-wind
    ;; Continuation predicates and delimited control (extensions)
    continuation?
    make-continuation-prompt-tag continuation-prompt-tag?
    default-continuation-prompt-tag
    call-with-continuation-prompt abort-current-continuation

    ;; === Exceptions (§6.11) ===
    error
    error-object? error-object-message error-object-irritants
    file-error? read-error?
    raise raise-continuable
    with-exception-handler
    ;; guard is a macro defined in base/exceptions.scm
    guard

    ;; === Input and output (§6.13) ===
    ;; Ports
    port?
    input-port? output-port?
    textual-port? binary-port?
    input-port-open? output-port-open?
    current-input-port current-output-port current-error-port
    ;; Port operations
    close-port close-input-port close-output-port
    call-with-port
    ;; String/bytevector ports
    open-input-string open-output-string get-output-string
    open-input-bytevector open-output-bytevector get-output-bytevector
    ;; Input
    read-char peek-char read-line char-ready?
    read-u8 peek-u8 u8-ready?
    read-string read-bytevector read-bytevector!
    ;; Output
    newline
    write-char write-string
    write-u8 write-bytevector
    flush-output-port
    ;; EOF
    eof-object eof-object?
    ;; Read/write/display (basic)
    read write display

    ;; === System interface (§6.14) ===
    features

    ;; === Dynamic bindings (§4.2.6) ===
    make-parameter
    parameterize  ;; macro defined in base/parameters.scm

    ;; === Record types (§5.5) ===
    ;; Internal primitives for define-record-type macro
    %make-record-type %record-type? %record?
    %record-type-of %record-type-name %record-type-fields
    %record-type-field-index
    %make-record %record-ref %record-set!

    ;; ============================================================
    ;; Derived forms (defined in Scheme via include)
    ;; ============================================================

    ;; From base/lists.scm
    not
    caar cadr cdar cddr
    caaar caadr cadar caddr
    cdaar cdadr cddar cdddr

    ;; From base/numbers.scm
    zero? positive? negative? odd? even?

    ;; From base/conditionals.scm
    when unless
    and or
    cond case

    ;; From base/binding.scm
    let let* letrec letrec*
    let-values let*-values
    define-values

    ;; From base/iteration.scm
    do

    ;; From base/records.scm
    define-record-type
    ;; Internal: needed for define-record-type macro expansion
    %make-constructor %define-field-accessors

    ;; === Auxiliary syntax (§4.3.2) ===
    ;; These must be exported so that (import (only (scheme base) else))
    ;; works and so syntax-rules literal matching works with renamed imports.
    ;; R7RS: "Any use as an independent syntactic construct or variable is an error."
    ;; Note: _ and ... have built-in meaning in syntax-rules and cannot be
    ;; defined as variables without breaking custom-ellipsis patterns (SRFI-46).
    else =>
  )

  ;; Auxiliary syntax bindings
  (begin
    (define else 'else)
    (define => '=>))

  ;; Include Scheme-defined derived forms (organized by domain)
  (include "base/lists.scm")
  (include "base/numbers.scm")
  (include "base/conditionals.scm")
  (include "base/binding.scm")
  (include "base/iteration.scm")
  (include "base/records.scm")
  (include "base/higher_order.scm")
  (include "base/exceptions.scm")
  (include "base/parameters.scm"))
