# Library System: Migration to .sld Format

**Created:** 2025-12-06
**Updated:** 2025-12-08
**Status:** Ready for Implementation
**Priority:** High - Prerequisite for finishing R7RS compliance work

---

## Executive Summary

Migrate Patina's library system to use proper `.sld` files for all R7RS standard libraries, with domain-specific internal libraries `(patina internal ...)` that group related primitives together regardless of which R7RS library exports them.

**Key Design Decision:** Internal libraries are organized by **domain** (numbers, lists, strings), not by R7RS library structure. This allows clean separation of concerns and makes it easy for multiple R7RS libraries to share primitives from the same domain.

---

## Target Architecture

### Library Hierarchy

```
┌─────────────────────────────────────────────────────────────────────────┐
│                       User Code                                          │
│           (import (scheme base) (scheme inexact))                        │
└────────────────────────────┬────────────────────────────────────────────┘
                             │
              ┌──────────────┴──────────────┐
              │                             │
              ▼                             ▼
┌─────────────────────────┐   ┌─────────────────────────┐
│   lib/scheme/base.sld   │   │  lib/scheme/inexact.sld │
│   (define-library        │   │  (define-library        │
│     (scheme base)        │   │    (scheme inexact)     │
│     (import              │   │    (import              │
│       (patina internal   │   │      (patina internal   │
│         numbers)         │   │        numbers))        │
│       (patina internal   │   │    (export sqrt sin...))│
│         lists)           │   └─────────────────────────┘
│       ...)               │
│     (export + - car...)) │
└─────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    Domain-Specific Internal Libraries                    │
│  (patina internal numbers) (patina internal lists) (patina internal io) │
│                         [Rust Primitives]                                │
└─────────────────────────────────────────────────────────────────────────┘
```

### Internal Library Organization (By Domain)

Maps to R7RS sections where applicable.

| Library | R7RS § | Contents | Used By |
|---------|--------|----------|---------|
| `(patina internal numbers)` | §6.2 | `+`, `-`, `*`, `/`, `=`, `<`, `>`, `floor`, `sqrt`, `sin`, `cos`, `exp`, `log`, `make-rectangular`, `real-part`, etc. | `(scheme base)`, `(scheme inexact)`, `(scheme complex)` |
| `(patina internal lists)` | §6.4 | `cons`, `car`, `cdr`, `list`, `append`, `memq`, `assq`, etc. | `(scheme base)` |
| `(patina internal chars)` | §6.6 | `char?`, `char->integer`, `integer->char`, `char=?`, `char<?`, `char-upcase`, `char-downcase`, `char-alphabetic?`, etc. | `(scheme base)`, `(scheme char)` |
| `(patina internal strings)` | §6.7 | `string?`, `string-length`, `string-ref`, `string-set!`, `make-string`, `substring`, `string-map`, `string-upcase`, `string-ci=?`, etc. | `(scheme base)`, `(scheme char)` |
| `(patina internal vectors)` | §6.8 | `vector?`, `make-vector`, `vector-ref`, `vector-set!`, `vector-map`, `vector-copy`, etc. | `(scheme base)` |
| `(patina internal bytevectors)` | §6.9 | `bytevector?`, `make-bytevector`, `bytevector-u8-ref`, `bytevector-u8-set!`, `utf8->string`, etc. | `(scheme base)` |
| `(patina internal control)` | §6.10 | `procedure?`, `apply`, `map`, `for-each`, `call/cc`, `values`, `call-with-values`, `dynamic-wind` | `(scheme base)` |
| `(patina internal errors)` | §6.11 | `error`, `error-object?`, `error-object-message`, `error-object-irritants`, `raise`, `raise-continuable`, `with-exception-handler` (stubs) | `(scheme base)` |
| `(patina internal io)` | §6.13 | `read`, `write`, `display`, `read-char`, `write-char`, port operations, etc. | `(scheme base)`, `(scheme read)`, `(scheme write)`, `(scheme file)` |
| `(patina internal predicates)` | §6.1, §6.3 | `boolean?`, `boolean=?`, `not`, `symbol?`, `symbol=?`, `symbol->string`, `string->symbol`, `pair?`, `null?`, `list?`, `eq?`, `eqv?`, `equal?` | `(scheme base)` |
| `(patina internal records)` | §5.5 | `%make-record-type`, `%record?`, `%record-ref`, `%record-set!`, etc. | `(scheme base)` |
| `(patina internal params)` | §4.2.6 | `make-parameter` | `(scheme base)` |
| `(patina internal time)` | - | `current-second`, `current-jiffy`, `jiffies-per-second` | `(scheme time)` |
| `(patina internal system)` | - | `features`, `command-line`, `exit`, `get-environment-variable` | `(scheme base)`, `(scheme process-context)` |

**Notes:**
- `map` and `for-each` are in §6.10 (Control features), not §6.4 (Lists), so they go in `(patina internal control)`
- Character comparison (`char=?`, etc.) and case conversion (`char-upcase`, etc.) are in §6.6, so they go in `(patina internal chars)`
- String comparison (`string=?`, `string-ci=?`, etc.) and case conversion (`string-upcase`, etc.) are in §6.7, so they go in `(patina internal strings)`
- `(scheme char)` imports from both `(patina internal chars)` and `(patina internal strings)` for full char library support

### File Structure

```
crates/patina-runtime/src/stdlib/
├── mod.rs                        # Registry of internal library builders
├── internal_numbers.rs           # (patina internal numbers) - §6.2
├── internal_lists.rs             # (patina internal lists) - §6.4
├── internal_chars.rs             # (patina internal chars) - §6.6
├── internal_strings.rs           # (patina internal strings) - §6.7
├── internal_vectors.rs           # (patina internal vectors) - §6.8
├── internal_bytevectors.rs       # (patina internal bytevectors) - §6.9
├── internal_control.rs           # (patina internal control) - §6.10
├── internal_errors.rs            # (patina internal errors) - §6.11 (stubs)
├── internal_io.rs                # (patina internal io) - §6.13
├── internal_predicates.rs        # (patina internal predicates) - §6.1, §6.3
├── internal_records.rs           # (patina internal records) - §5.5
├── internal_params.rs            # (patina internal params) - §4.2.6
├── internal_time.rs              # (patina internal time)
├── internal_system.rs            # (patina internal system)
└── patina_debug.rs               # (patina debug) - keep as user-facing

lib/
├── scheme/
│   ├── base.sld                  # (scheme base)
│   ├── base/
│   │   ├── lists.scm             # caar, cadr, not, list predicates
│   │   ├── numbers.scm           # zero?, positive?, negative?, odd?, even?
│   │   ├── binding.scm           # let, let*, letrec, letrec*, let-values
│   │   ├── conditionals.scm      # cond, case, and, or, when, unless
│   │   ├── iteration.scm         # do
│   │   └── records.scm           # define-record-type
│   ├── char.sld                  # (scheme char)
│   ├── complex.sld               # (scheme complex)
│   ├── inexact.sld               # (scheme inexact)
│   ├── lazy.sld                  # (scheme lazy)
│   ├── lazy/
│   │   └── promises.scm          # delay-force macro
│   ├── case-lambda.sld           # (scheme case-lambda) - already exists
│   ├── read.sld                  # (scheme read)
│   ├── write.sld                 # (scheme write)
│   ├── file.sld                  # (scheme file)
│   ├── time.sld                  # (scheme time)
│   ├── process-context.sld       # (scheme process-context)
│   ├── cxr.sld                   # (scheme cxr)
│   ├── eval.sld                  # (scheme eval)
│   └── r5rs.sld                  # (scheme r5rs)
├── chibi/
│   └── test.sld                  # (chibi test) - already exists
└── srfi/
    └── (future SRFI libraries)
```

---

## Detailed Design

### Example: `(patina internal numbers)` (Rust)

```rust
// crates/patina-runtime/src/stdlib/internal_numbers.rs

pub fn build_internal_numbers(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["patina".into(), "internal".into(), "numbers".into()];

    let primitives = [
        // Basic arithmetic (scheme base)
        ("+", Arity::Min(0)),
        ("-", Arity::Min(1)),
        ("*", Arity::Min(0)),
        ("/", Arity::Min(1)),
        ("=", Arity::Min(2)),
        ("<", Arity::Min(2)),
        (">", Arity::Min(2)),
        ("<=", Arity::Min(2)),
        (">=", Arity::Min(2)),
        ("quotient", Arity::Exact(2)),
        ("remainder", Arity::Exact(2)),
        ("modulo", Arity::Exact(2)),
        ("floor/", Arity::Exact(2)),
        ("floor-quotient", Arity::Exact(2)),
        ("floor-remainder", Arity::Exact(2)),
        ("truncate/", Arity::Exact(2)),
        ("truncate-quotient", Arity::Exact(2)),
        ("truncate-remainder", Arity::Exact(2)),
        ("abs", Arity::Exact(1)),
        ("max", Arity::Min(1)),
        ("min", Arity::Min(1)),
        ("floor", Arity::Exact(1)),
        ("ceiling", Arity::Exact(1)),
        ("truncate", Arity::Exact(1)),
        ("round", Arity::Exact(1)),
        ("square", Arity::Exact(1)),
        ("expt", Arity::Exact(2)),
        ("gcd", Arity::Min(0)),
        ("lcm", Arity::Min(0)),
        ("numerator", Arity::Exact(1)),
        ("denominator", Arity::Exact(1)),
        ("exact", Arity::Exact(1)),
        ("inexact", Arity::Exact(1)),
        ("exact-integer-sqrt", Arity::Exact(1)),
        ("rationalize", Arity::Exact(2)),
        ("number->string", Arity::Range(1, 2)),
        ("string->number", Arity::Range(1, 2)),

        // Inexact operations (scheme inexact)
        ("sqrt", Arity::Exact(1)),
        ("exp", Arity::Exact(1)),
        ("log", Arity::Range(1, 2)),
        ("sin", Arity::Exact(1)),
        ("cos", Arity::Exact(1)),
        ("tan", Arity::Exact(1)),
        ("asin", Arity::Exact(1)),
        ("acos", Arity::Exact(1)),
        ("atan", Arity::Range(1, 2)),
        ("finite?", Arity::Exact(1)),
        ("infinite?", Arity::Exact(1)),
        ("nan?", Arity::Exact(1)),

        // Complex operations (scheme complex)
        ("make-rectangular", Arity::Exact(2)),
        ("make-polar", Arity::Exact(2)),
        ("real-part", Arity::Exact(1)),
        ("imag-part", Arity::Exact(1)),
        ("magnitude", Arity::Exact(1)),
        ("angle", Arity::Exact(1)),
    ];

    for (name, arity) in &primitives {
        env.define(name.to_string(), Value::Procedure(Rc::new(Procedure::Primitive {
            name,
            arity: arity.clone(),
            library: library_name.clone(),
        })));
    }

    primitives.iter().map(|(n, _)| n.to_string()).collect()
}
```

### Example: `lib/scheme/base.sld`

```scheme
(define-library (scheme base)
  ;; Import from domain-specific internal libraries (ordered by R7RS section)
  (import (patina internal predicates)   ; §6.1 Booleans, §6.3 Symbols, §6.4 (pair?/null?)
          (patina internal numbers)      ; §6.2 Numbers
          (patina internal lists)        ; §6.4 Pairs and lists
          (patina internal chars)        ; §6.6 Characters
          (patina internal strings)      ; §6.7 Strings
          (patina internal vectors)      ; §6.8 Vectors
          (patina internal bytevectors)  ; §6.9 Bytevectors
          (patina internal control)      ; §6.10 Control (apply, map, for-each, values, call/cc)
          (patina internal errors)       ; §6.11 Exceptions (stubs)
          (patina internal io)           ; §6.13 I/O
          (patina internal records)      ; §5.5 Record types
          (patina internal params)       ; §4.2.6 Dynamic bindings
          (patina internal system))      ; features

  ;; R7RS (scheme base) exports
  (export
    ;; === Arithmetic (from internal numbers) ===
    + - * / = < > <= >=
    quotient remainder modulo
    floor/ floor-quotient floor-remainder
    truncate/ truncate-quotient truncate-remainder
    abs max min
    floor ceiling truncate round
    square expt
    gcd lcm
    numerator denominator
    exact inexact
    exact-integer-sqrt rationalize
    number->string string->number

    ;; === Predicates (from internal predicates) ===
    number? complex? real? rational? integer?
    exact? inexact? exact-integer?
    boolean? string? symbol? null? pair? list?
    procedure? char? vector? bytevector?
    eq? eqv? equal?
    boolean=? symbol=?

    ;; === Lists (from internal lists) ===
    cons car cdr
    set-car! set-cdr!
    list make-list length
    append reverse list-tail list-ref list-set!
    list-copy
    memq memv member
    assq assv assoc

    ;; === Strings (from internal strings) ===
    string? make-string string string-length
    string-ref string-set!
    string=? string<? string>? string<=? string>=?
    substring string-append
    string->list list->string
    string-copy string-copy! string-fill!
    string-for-each string-map

    ;; === Vectors (from internal vectors) ===
    vector? make-vector vector vector-length
    vector-ref vector-set!
    vector->list list->vector
    vector->string string->vector
    vector-copy vector-copy! vector-fill!
    vector-append
    vector-for-each vector-map

    ;; === Bytevectors (from internal bytevectors) ===
    bytevector? make-bytevector bytevector
    bytevector-length
    bytevector-u8-ref bytevector-u8-set!
    bytevector-copy bytevector-copy!
    bytevector-append
    utf8->string string->utf8

    ;; === Characters (from internal chars) ===
    char? char->integer integer->char
    char=? char<? char>? char<=? char>=?

    ;; === I/O (from internal io) ===
    port? input-port? output-port?
    textual-port? binary-port?
    input-port-open? output-port-open?
    current-input-port current-output-port current-error-port
    close-port close-input-port close-output-port
    open-input-string open-output-string get-output-string
    open-input-bytevector open-output-bytevector get-output-bytevector
    read-char peek-char read-line char-ready?
    read-u8 peek-u8 u8-ready?
    read-string read-bytevector read-bytevector!
    write-char write-string write-u8 write-bytevector
    newline flush-output-port
    call-with-port
    eof-object eof-object?
    read write display

    ;; === Control (from internal control) ===
    procedure? apply
    map for-each
    values call-with-values
    call-with-current-continuation call/cc
    dynamic-wind

    ;; === Errors (from internal errors) - stubs ===
    error
    error-object? error-object-message error-object-irritants
    file-error? read-error?
    raise raise-continuable
    with-exception-handler

    ;; === Parameters (from internal params) ===
    make-parameter

    ;; === Records (internal primitives, not exported directly) ===
    ;; Record primitives (%make-record-type, etc.) are used by define-record-type macro

    ;; === System (from internal system) ===
    features

    ;; === Symbols (from internal predicates) ===
    symbol->string string->symbol

    ;; === Derived forms (from included Scheme files) ===
    ;; List accessors
    caar cadr cdar cddr
    caaar caadr cadar caddr cdaar cdadr cddar cdddr
    not

    ;; Numeric predicates
    zero? positive? negative? odd? even?

    ;; Binding forms
    let let* letrec letrec*
    let-values let*-values define-values

    ;; Conditionals
    cond case and or when unless

    ;; Iteration
    do

    ;; Records
    define-record-type

    ;; Exception handling (macro)
    guard
  )

  ;; Include Scheme-defined derived forms (organized by domain)
  (include "base/lists.scm")
  (include "base/numbers.scm")
  (include "base/binding.scm")
  (include "base/conditionals.scm")
  (include "base/iteration.scm")
  (include "base/records.scm"))
```

### Example: `lib/scheme/inexact.sld`

```scheme
(define-library (scheme inexact)
  ;; Import only what we need from internal numbers
  (import (only (patina internal numbers)
                sqrt exp log sin cos tan asin acos atan
                finite? infinite? nan?))

  (export sqrt exp log sin cos tan asin acos atan
          finite? infinite? nan?))
```

### Example: `lib/scheme/complex.sld`

```scheme
(define-library (scheme complex)
  (import (only (patina internal numbers)
                make-rectangular make-polar
                real-part imag-part magnitude angle))

  (export make-rectangular make-polar
          real-part imag-part magnitude angle))
```

### Example: `lib/scheme/char.sld`

```scheme
(define-library (scheme char)
  (import (only (patina internal chars)
                char-alphabetic? char-numeric? char-whitespace?
                char-upper-case? char-lower-case?
                digit-value
                char-upcase char-downcase char-foldcase
                char-ci=? char-ci<? char-ci>? char-ci<=? char-ci>=?
                string-ci=? string-ci<? string-ci>? string-ci<=? string-ci>=?
                string-upcase string-downcase string-foldcase))

  (export char-alphabetic? char-numeric? char-whitespace?
          char-upper-case? char-lower-case?
          digit-value
          char-upcase char-downcase char-foldcase
          char-ci=? char-ci<? char-ci>? char-ci<=? char-ci>=?
          string-ci=? string-ci<? string-ci>? string-ci<=? string-ci>=?
          string-upcase string-downcase string-foldcase))
```

---

## Implementation Plan

This is a **big-bang migration** - we convert everything at once rather than incrementally. The goal is to eliminate all direct `.scm` loading; everything goes through `.sld` files.

### Phase 1: Create Internal Libraries (Rust)

Split `scheme_base.rs` and other stdlib files into domain-specific internal libraries, organized by R7RS section:

| # | File | Library | R7RS § | Source |
|---|------|---------|--------|--------|
| 1 | `internal_numbers.rs` | `(patina internal numbers)` | §6.2 | `scheme_base.rs` arithmetic + `scheme_inexact.rs` + `scheme_complex.rs` |
| 2 | `internal_lists.rs` | `(patina internal lists)` | §6.4 | `scheme_base.rs` list operations |
| 3 | `internal_chars.rs` | `(patina internal chars)` | §6.6 | `scheme_base.rs` char ops + `scheme_char.rs` |
| 4 | `internal_strings.rs` | `(patina internal strings)` | §6.7 | `scheme_base.rs` string operations |
| 5 | `internal_vectors.rs` | `(patina internal vectors)` | §6.8 | `scheme_base.rs` vector operations |
| 6 | `internal_bytevectors.rs` | `(patina internal bytevectors)` | §6.9 | `scheme_base.rs` bytevector operations |
| 7 | `internal_control.rs` | `(patina internal control)` | §6.10 | `scheme_base.rs`: `procedure?`, `apply`, `map`, `for-each`, `values`, `call-with-values` |
| 8 | `internal_errors.rs` | `(patina internal errors)` | §6.11 | **Stubs**: `error`, `raise`, `with-exception-handler`, etc. |
| 9 | `internal_io.rs` | `(patina internal io)` | §6.13 | `scheme_base.rs` I/O + `scheme_read.rs` + `scheme_write.rs` + `scheme_file.rs` |
| 10 | `internal_predicates.rs` | `(patina internal predicates)` | §6.1, §6.3 | `scheme_base.rs`: `eq?`, `eqv?`, `equal?`, `boolean?`, `symbol?`, `pair?`, `null?`, `list?` |
| 11 | `internal_records.rs` | `(patina internal records)` | §5.5 | `scheme_base.rs`: `%make-record-type`, `%record-ref`, etc. |
| 12 | `internal_params.rs` | `(patina internal params)` | §4.2.6 | `scheme_base.rs`: `make-parameter` |
| 13 | `internal_time.rs` | `(patina internal time)` | - | `scheme_time.rs` |
| 14 | `internal_system.rs` | `(patina internal system)` | - | `scheme_base.rs`: `features` + `scheme_process_context.rs` |

Register all as `(patina internal ...)` in `RustLibraryLoader`.

### Phase 2: Create .sld Files and Split Scheme Code

**2.1: Create `lib/scheme/base.sld`**
- Imports from all internal libraries
- Explicit exports for R7RS (scheme base)
- Uses `(include ...)` for derived forms

**2.2: Split `base-extras.scm` by domain:**

| File | Contents |
|------|----------|
| `lib/scheme/base/lists.scm` | `caar`, `cadr`, `cdar`, `cddr`, `caaar`..., `not` |
| `lib/scheme/base/numbers.scm` | `zero?`, `positive?`, `negative?`, `odd?`, `even?` |
| `lib/scheme/base/binding.scm` | `let`, `let*`, `letrec`, `letrec*`, `let-values`, `let*-values`, `define-values` |
| `lib/scheme/base/conditionals.scm` | `cond`, `case`, `and`, `or`, `when`, `unless` |
| `lib/scheme/base/iteration.scm` | `do` |
| `lib/scheme/base/records.scm` | `define-record-type` |
| `lib/scheme/base/errors.scm` | `guard` (once exceptions work) |

**2.3: Create `.sld` files for other R7RS libraries:**

| Library | Source | Notes |
|---------|--------|-------|
| `lib/scheme/char.sld` | `(patina internal chars)` + `(patina internal strings)` | Case conversion, char predicates |
| `lib/scheme/complex.sld` | `(patina internal numbers)` | `make-rectangular`, `real-part`, etc. |
| `lib/scheme/inexact.sld` | `(patina internal numbers)` | `sqrt`, `sin`, `cos`, etc. |
| `lib/scheme/lazy.sld` | `(patina internal control)` | `delay`, `force`, `delay-force`, etc. |
| `lib/scheme/read.sld` | `(patina internal io)` | `read` |
| `lib/scheme/write.sld` | `(patina internal io)` | `write`, `write-shared`, `write-simple`, `display` |
| `lib/scheme/file.sld` | `(patina internal io)` | File operations |
| `lib/scheme/time.sld` | `(patina internal time)` | Time operations |
| `lib/scheme/process-context.sld` | `(patina internal system)` | `command-line`, `exit`, etc. |
| `lib/scheme/cxr.sld` | `(patina internal lists)` | Extended car/cdr (pure Scheme) |
| `lib/scheme/eval.sld` | TBD | `eval`, `environment` |
| `lib/scheme/r5rs.sld` | Multiple | R5RS compatibility |
| `lib/scheme/case-lambda.sld` | Already exists | Keep as-is |

### Phase 3: Update Loader System

1. **Remove** all `(scheme *)` registrations from `RustLibraryLoader`
2. **Keep** only `(patina internal *)` and `(patina debug)` in Rust loader
3. **Remove** `load_library_extras()` mechanism entirely - no more `*-extras.scm` convention
4. **All** `.scm` files loaded only via `(include ...)` in `.sld` files
5. **Update** loader routing:
   - `(patina *)` → `RustLibraryLoader`
   - Everything else → `SchemeLibraryLoader`

### Phase 4: Cleanup and Testing

1. **Delete** old files:
   - `scheme_base.rs`, `scheme_char.rs`, `scheme_complex.rs`, `scheme_inexact.rs`, etc.
   - `base-extras.scm`, `lazy-extras.scm`
2. **Update** tests to use new library structure
3. **Verify** all ~794 tests still pass
4. **Run** chibi r7rs-tests.scm compatibility check
5. **Update** CLAUDE.md and other documentation

---

## Loader Behavior

### After Migration

```
(import (scheme base))
  → SchemeLibraryLoader finds lib/scheme/base.sld
  → Parses define-library form
  → Processes (import (patina internal numbers) ...)
    → RustLibraryLoader handles (patina internal numbers)
    → ... handles other internal libraries
  → Processes (include "base/lists.scm" ...)
    → Loads and evaluates Scheme files
  → Registers exports

(import (patina internal numbers))
  → RustLibraryLoader handles directly (Rust primitives)

(import (patina debug))
  → RustLibraryLoader handles directly (user-facing utility)
```

### Loader Priority Rules

| Library Pattern | Handler |
|-----------------|---------|
| `(patina internal *)` | RustLibraryLoader |
| `(patina debug)` | RustLibraryLoader |
| `(scheme *)` | SchemeLibraryLoader → .sld files |
| `(chibi *)` | SchemeLibraryLoader → .sld files |
| `(srfi *)` | SchemeLibraryLoader → .sld files |

---

## Open Issues / Future Work

### TODO: Library-Scoped Special Forms

Currently special forms (`quote`, `if`, `lambda`, `define`, `set!`, `begin`, `define-syntax`, `let-syntax`, `letrec-syntax`) are globally registered in the evaluator. This is not R7RS compliant - they should only be available after importing `(scheme base)`.

**Deferred to future work.** For now, special forms remain global. This doesn't affect most user code but is technically non-compliant.

Potential solutions:
- Check environment for special form bindings before applying
- Rethink evaluator architecture to make special forms library-scoped
- Add special form "values" to `(patina internal syntax)` that enable them

### TODO: `cond-expand` and `include-ci`

These library declaration forms need implementation:
- `cond-expand` - conditional expansion based on features
- `include-ci` - case-insensitive include

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_internal_numbers_exports() {
    // Verify (patina internal numbers) exports all expected primitives
}

#[test]
fn test_scheme_base_loads_from_sld() {
    // Verify (scheme base) loads via .sld file
    // Check all exports are available
}

#[test]
fn test_import_chain() {
    // (scheme base) → (patina internal numbers) works
}
```

### Integration Tests

```rust
#[test]
fn test_no_direct_scm_loading() {
    // Verify no .scm files are loaded directly
    // All Scheme code loaded via (include ...) in .sld files
}

#[test]
fn test_r7rs_library_compliance() {
    // Run chibi r7rs-tests.scm
    // Compare results before/after migration
}
```

---

## Benefits

| Benefit | Description |
|---------|-------------|
| **Clean domain separation** | Numbers together, lists together, regardless of R7RS library |
| **R7RS compliance** | Standard `.sld` format for all user-facing libraries |
| **Explicit exports** | All exports visible in `.sld` files |
| **No magic loading** | No `*-extras.scm` convention, everything via `(include ...)` |
| **Composable** | R7RS libraries compose internal libraries cleanly |
| **Easy to extend** | Add new R7RS libraries by creating `.sld` files |
| **Matches community patterns** | Similar to chibi-scheme architecture |

---

## References

- Chibi-scheme: `lib/scheme/base.sld`
- R7RS §5.6.1: Library declarations
- Current Patina: `crates/patina-runtime/src/stdlib/`
