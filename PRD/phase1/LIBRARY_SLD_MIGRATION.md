# Library System: Migration to .sld Format

**Created:** 2025-12-06
**Status:** Research
**Priority:** Medium - Improves R7RS compliance and code organization

---

## Executive Summary

Patina currently uses a non-standard hybrid approach for standard libraries: Rust primitives are registered directly as `(scheme base)`, with Scheme extras loaded from `*-extras.scm` files. This research document analyzes chibi-scheme's approach and proposes migrating to proper `.sld` files for all R7RS standard libraries.

**Key Finding:** Chibi-scheme uses internal libraries (e.g., `(chibi)`) for native primitives, then defines R7RS libraries (e.g., `(scheme base)`) as `.sld` files that import from internal libraries. This is cleaner, more R7RS-compliant, and easier to understand.

---

## Current Patina Architecture

### How Libraries Are Loaded Today

```
┌─────────────────────────────────────────────────────────────────┐
│                  (import (scheme base))                          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │   RustLibraryLoader          │
              │   (highest priority)         │
              └──────────────┬───────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │   build_scheme_base()        │
              │   (Rust primitives)          │
              │   Creates: +, -, car, cdr... │
              └──────────────┬───────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │   load_library_extras()      │
              │   Loads: base-extras.scm     │
              │   Adds: let, cond, case...   │
              └──────────────────────────────┘
```

### Current File Structure

```
crates/patina-runtime/src/stdlib/
├── mod.rs                  # Registry of Rust library builders
├── scheme_base.rs          # (scheme base) primitives
├── scheme_char.rs          # (scheme char) primitives
├── scheme_complex.rs       # (scheme complex) primitives
├── scheme_inexact.rs       # (scheme inexact) primitives
├── scheme_lazy.rs          # (scheme lazy) primitives
└── ...

lib/scheme/
├── base-extras.scm         # Macros for (scheme base)
└── lazy-extras.scm         # Extras for (scheme lazy)
```

### How `build_scheme_base()` Works

```rust
// crates/patina-runtime/src/stdlib/scheme_base.rs
pub fn build_scheme_base(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "base".to_string()];

    let primitives = [
        ("+", Arity::Min(0)),
        ("-", Arity::Min(1)),
        ("car", Arity::Exact(1)),
        ("cdr", Arity::Exact(1)),
        // ... 150+ primitives
    ];

    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Procedure::Primitive {
                name: (*name).into(),
                arity: arity.clone(),
                library: library_name.clone(),
            }),
        );
    }

    // Return export list
    primitives.iter().map(|(name, _)| name.to_string()).collect()
}
```

### How `load_library_extras()` Works

```rust
// crates/patina-tree-walker/src/eval/mod.rs
fn load_library_extras(&self, name: &[String]) {
    // Build path: lib/scheme/base-extras.scm
    let mut relative_path = PathBuf::new();
    for part in &name[..name.len() - 1] {
        relative_path.push(part);
    }
    relative_path.push(format!("{}-extras.scm", name.last().unwrap()));

    // Find and load the file
    let extras_path = self.find_in_search_paths(&relative_path);
    let content = std::fs::read_to_string(&extras_path)?;

    // Evaluate in library's environment
    // ... parse and eval each expression
}
```

### Problems with Current Approach

1. **Non-standard format** - `*-extras.scm` files are a Patina-specific convention
2. **Implicit loading** - Magic happens in `load_library_extras()`, not visible to users
3. **No explicit exports** - Extras file doesn't declare what it exports
4. **Tight coupling** - Rust and Scheme parts are implicitly linked by naming convention
5. **Hard to understand** - New contributors must learn the non-standard pattern
6. **R7RS non-compliant: Special forms are globally available**
   - `case-lambda` works without `(import (scheme case-lambda))`
   - All special forms (`define`, `lambda`, `if`, etc.) are always available
   - R7RS requires importing the appropriate library to access its exports

### R7RS Compliance Issue: Global Special Forms

Currently, Patina registers special forms globally in the evaluator:

```rust
// Special forms are always available - NOT R7RS compliant
evaluator.register_special_form("case-lambda", ...);
evaluator.register_special_form("define", ...);
evaluator.register_special_form("lambda", ...);
```

This means:
```scheme
;; This works in Patina but SHOULD fail per R7RS:
(case-lambda (() 0) ((x) x))  ; No import!

;; R7RS requires:
(import (scheme case-lambda))
(case-lambda (() 0) ((x) x))  ; Now it works
```

**How chibi handles this:** `case-lambda` is a macro in `(srfi 16)`, not a special form:

```scheme
;; chibi's lib/srfi/16.sld
(define-library (srfi 16)
  (export case-lambda)
  (import (chibi))
  (begin
    (define-syntax case-lambda
      (syntax-rules ()
        ((case-lambda . clauses)
         (lambda args ...))))))

;; (scheme case-lambda) is just an alias
(define-library (scheme case-lambda) (alias-for (srfi 16)))
```

**Implication:** Many of Patina's "special forms" should be macros:

| True Special Forms (evaluator) | Should Be Macros (library) |
|-------------------------------|---------------------------|
| `quote`, `if`, `lambda` | `case-lambda` |
| `define`, `define-syntax` | `let`, `let*`, `letrec` |
| `set!`, `begin` | `cond`, `case`, `and`, `or` |
| `syntax-rules` | `when`, `unless`, `do`, `guard` |

This means `(patina core)` should only provide the minimal true special forms, and everything else should be macros defined in `.sld` files.

---

## Chibi-Scheme Architecture

### How Chibi Structures Libraries

```
┌─────────────────────────────────────────────────────────────────┐
│                  (import (scheme base))                          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │   SchemeLibraryLoader        │
              │   Finds: lib/scheme/base.sld │
              └──────────────┬───────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │   lib/scheme/base.sld        │
              │   (define-library ...)       │
              │   (import (chibi))           │
              │   (include "extras.scm")     │
              └──────────────────────────────┘
                             │
            ┌────────────────┴────────────────┐
            │                                 │
            ▼                                 ▼
┌───────────────────────┐      ┌─────────────────────────────┐
│   (chibi)             │      │   lib/scheme/extras.scm     │
│   C primitives        │      │   Scheme macros/procedures  │
│   (native code)       │      │                             │
└───────────────────────┘      └─────────────────────────────┘
```

### Chibi's `lib/scheme/base.sld`

```scheme
(define-library (scheme base)
  ;; Import from chibi's internal libraries
  (import (rename (except (chibi) equal?)
                  (let-syntax let-syntax/splicing)
                  (letrec-syntax letrec-syntax/splicing))
          (rename (chibi equiv) (equiv? equal?))
          (only (chibi string) string-map string-for-each)
          (chibi io)
          (rename (only (chibi ast)
                        exception? exception-message exception-irritants)
                  (exception? error-object?)
                  (exception-message error-object-message)
                  (exception-irritants error-object-irritants))
          (srfi 9) (srfi 11) (srfi 39))

  ;; Explicit R7RS exports
  (export
   * + - ... / < <= = => > >= _
   abs and append apply assoc assq assv begin
   car cdr cons
   cond case define define-syntax
   if lambda let let* letrec
   ;; ... full R7RS export list
   )

  ;; Additional Scheme code
  (include "define-values.scm"
           "extras.scm"
           "misc-macros.scm"))
```

### Key Insights from Chibi

1. **Internal vs Standard Libraries**
   - `(chibi)` = Internal library with C primitives
   - `(scheme base)` = R7RS standard library (`.sld` file)

2. **Explicit Imports/Exports**
   - `.sld` files explicitly import from internal libraries
   - All exports are listed in the `export` declaration

3. **Composition via Import Modifiers**
   - Uses `rename`, `except`, `only` to shape the API
   - Can pull from multiple internal libraries

4. **Include for Scheme Code**
   - Macros and procedures defined in separate `.scm` files
   - Included via standard `include` declaration

---

## Proposed Patina Architecture

### New Library Structure

```
┌─────────────────────────────────────────────────────────────────┐
│                  (import (scheme base))                          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │   SchemeLibraryLoader        │
              │   Finds: lib/scheme/base.sld │
              └──────────────┬───────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │   lib/scheme/base.sld        │
              │   (define-library ...)       │
              │   (import (patina core))     │
              │   (include "base-macros.scm")│
              └──────────────────────────────┘
                             │
            ┌────────────────┴────────────────┐
            │                                 │
            ▼                                 ▼
┌───────────────────────┐      ┌─────────────────────────────┐
│   (patina core)       │      │   lib/scheme/base-macros.scm│
│   Rust primitives     │      │   let, cond, case, and, or  │
│   (RustLibraryLoader) │      │   do, when, unless, etc.    │
└───────────────────────┘      └─────────────────────────────┘
```

### New File Structure

```
crates/patina-runtime/src/stdlib/
├── mod.rs
├── patina_core.rs          # (patina core) - all Rust primitives
├── patina_char.rs          # (patina char) - char primitives
├── patina_io.rs            # (patina io) - I/O primitives
└── ...

lib/
├── scheme/
│   ├── base.sld            # (scheme base) - R7RS library
│   ├── base-macros.scm     # Macros for (scheme base)
│   ├── char.sld            # (scheme char)
│   ├── complex.sld         # (scheme complex)
│   ├── inexact.sld         # (scheme inexact)
│   ├── lazy.sld            # (scheme lazy)
│   ├── lazy-macros.scm     # Macros for (scheme lazy)
│   ├── write.sld           # (scheme write)
│   └── ...
├── patina/
│   └── (internal libraries if needed)
└── srfi/
    └── (SRFI libraries)
```

### Example: `lib/scheme/base.sld`

```scheme
(define-library (scheme base)
  ;; Import Rust primitives from internal library
  (import (patina core))

  ;; R7RS (scheme base) exports - explicit and complete
  (export
    ;; Arithmetic
    + - * / = < > <= >=
    abs quotient remainder modulo
    floor ceiling truncate round
    sqrt square expt
    gcd lcm
    numerator denominator
    exact inexact
    exact-integer-sqrt rationalize
    max min
    floor/ floor-quotient floor-remainder
    truncate/ truncate-quotient truncate-remainder

    ;; Type predicates
    number? complex? real? rational? integer?
    exact? inexact? exact-integer?
    zero? positive? negative? odd? even?
    nan? infinite? finite?

    ;; Pairs and lists
    cons car cdr
    set-car! set-cdr!
    pair? null? list?
    list make-list length
    append reverse list-tail list-ref list-set!
    list-copy
    memq memv member
    assq assv assoc

    ;; cxr procedures (defined in macros)
    caar cadr cdar cddr
    caaar caadr cadar caddr
    cdaar cdadr cddar cdddr

    ;; Symbols
    symbol? symbol->string string->symbol symbol=?

    ;; Characters
    char? char->integer integer->char
    char=? char<? char>? char<=? char>=?

    ;; Strings
    string? make-string string string-length
    string-ref string-set!
    string=? string<? string>? string<=? string>=?
    substring string-append
    string->list list->string
    string-copy string-copy! string-fill!
    string-for-each string-map

    ;; Vectors
    vector? make-vector vector vector-length
    vector-ref vector-set!
    vector->list list->vector
    vector-copy vector-copy! vector-fill!
    vector-append
    vector-for-each vector-map

    ;; Bytevectors
    bytevector? make-bytevector bytevector
    bytevector-length
    bytevector-u8-ref bytevector-u8-set!
    bytevector-copy bytevector-copy!
    bytevector-append
    utf8->string string->utf8

    ;; Control
    procedure? apply
    map for-each
    call-with-current-continuation call/cc
    call-with-values values
    dynamic-wind

    ;; Exceptions
    error
    with-exception-handler
    raise raise-continuable
    error-object? error-object-message error-object-irritants
    file-error? read-error?

    ;; I/O
    input-port? output-port? port?
    textual-port? binary-port?
    input-port-open? output-port-open?
    current-input-port current-output-port current-error-port
    close-port close-input-port close-output-port
    open-input-string open-output-string get-output-string
    open-input-bytevector open-output-bytevector get-output-bytevector
    read-char peek-char read-line char-ready?
    read-u8 peek-u8 u8-ready?
    read-string read-bytevector read-bytevector!
    write-char write-string write-u8
    write-bytevector
    newline flush-output-port
    call-with-port
    eof-object eof-object?

    ;; Equality
    eq? eqv? equal?
    boolean? boolean=?
    not

    ;; Multiple values
    values call-with-values

    ;; Parameters
    make-parameter parameterize

    ;; Syntax (special forms)
    quote quasiquote unquote unquote-splicing
    lambda if set! define
    define-syntax let-syntax letrec-syntax
    syntax-rules syntax-error
    begin

    ;; Derived syntax (from macros)
    let let* letrec letrec*
    let-values let*-values
    cond case and or
    when unless
    do
    define-values
    define-record-type
    guard

    ;; System
    features

    ;; Include/cond-expand (library syntax, also expression)
    include include-ci
    cond-expand
    )

  ;; Include macro definitions
  (include "base-macros.scm"))
```

### Example: `lib/scheme/base-macros.scm`

```scheme
;; Rename current base-extras.scm content
;; This file contains all derived forms implemented as macros

;; cxr procedures
(define (caar x) (car (car x)))
(define (cadr x) (car (cdr x)))
(define (cdar x) (cdr (car x)))
(define (cddr x) (cdr (cdr x)))
;; ... etc

;; Boolean helpers
(define (not x) (if x #f #t))
(define (boolean=? . bools) ...)

;; Numeric predicates
(define (zero? x) (= x 0))
(define (positive? x) (> x 0))
(define (negative? x) (< x 0))
(define (odd? x) (= 1 (modulo x 2)))
(define (even? x) (= 0 (modulo x 2)))

;; let, let*, letrec, letrec*
(define-syntax let ...)
(define-syntax let* ...)
(define-syntax letrec ...)
(define-syntax letrec* ...)

;; cond, case, and, or
(define-syntax cond ...)
(define-syntax case ...)
(define-syntax and ...)
(define-syntax or ...)

;; when, unless
(define-syntax when ...)
(define-syntax unless ...)

;; do loop
(define-syntax do ...)

;; Multiple value binding
(define-syntax let-values ...)
(define-syntax let*-values ...)
(define-syntax define-values ...)

;; guard (exception handling)
(define-syntax guard ...)
```

### Internal Library: `(patina core)`

```rust
// crates/patina-runtime/src/stdlib/patina_core.rs

/// Build the (patina core) internal library
///
/// Contains ALL Rust-implemented primitives. This is the internal
/// foundation that R7RS libraries import from.
pub fn build_patina_core(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["patina".to_string(), "core".to_string()];

    let primitives = [
        // All primitives from current scheme_base.rs
        // Plus primitives from scheme_char.rs, scheme_complex.rs, etc.
        ("+", Arity::Min(0)),
        ("-", Arity::Min(1)),
        ("*", Arity::Min(0)),
        ("/", Arity::Min(1)),
        // ... everything
    ];

    // Register all primitives
    for (name, arity) in &primitives {
        env.define(name.to_string(), Value::Procedure(Procedure::Primitive {
            name: (*name).into(),
            arity: arity.clone(),
            library: library_name.clone(),
        }));
    }

    primitives.iter().map(|(name, _)| name.to_string()).collect()
}
```

---

## Migration Plan

### Phase 1: Create Internal Library (Low Risk)

1. Create `patina_core.rs` that exports all primitives
2. Register `(patina core)` in `RustLibraryLoader`
3. Keep existing `(scheme base)` working via Rust
4. **No breaking changes**

### Phase 2: Create .sld Files (Medium Risk)

1. Create `lib/scheme/base.sld` that imports `(patina core)`
2. Rename `base-extras.scm` to `base-macros.scm`
3. Update loader priority: SchemeLibraryLoader before RustLibraryLoader for `(scheme *)`
4. Test thoroughly

### Phase 3: Migrate Other Libraries

1. Create `.sld` files for: `char`, `complex`, `inexact`, `lazy`, `write`
2. Each imports from appropriate `(patina *)` internal library
3. Remove Rust library builders for `(scheme *)` names

### Phase 4: Cleanup

1. Remove `load_library_extras()` mechanism
2. Remove `*-extras.scm` naming convention
3. Update documentation

---

## Detailed Changes

### RustLibraryLoader Registration

```rust
// Before
loader.register("scheme.base", build_scheme_base);
loader.register("scheme.char", build_scheme_char);

// After
loader.register("patina.core", build_patina_core);
loader.register("patina.char", build_patina_char);
// (scheme base) is now loaded from .sld file
```

### Loader Priority

```rust
// Current: Rust first, then Scheme
loaders: [RustLibraryLoader, SchemeLibraryLoader]

// New: Need smarter routing
// Option A: Rust only for (patina *), Scheme for (scheme *)
// Option B: Scheme first for (scheme *), Rust first otherwise
```

### Library Search

```
(import (scheme base))
  → Look for lib/scheme/base.sld  ✓
  → Parse and load .sld file
  → .sld imports (patina core)
    → RustLibraryLoader handles (patina core)

(import (patina core))
  → RustLibraryLoader handles directly
```

---

## Benefits

| Benefit | Description |
|---------|-------------|
| **R7RS Compliance** | Standard libraries use standard `.sld` format |
| **Explicit Exports** | All exports visible in `.sld` file |
| **Understandable** | Matches how other R7RS implementations work |
| **Portable Patterns** | Users familiar with chibi/gauche can understand |
| **Separation of Concerns** | Clear split: Rust primitives vs Scheme derived forms |
| **Extensibility** | Easy to add new R7RS libraries as `.sld` files |
| **Debugging** | Can inspect `.sld` files to see what's exported |

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Breaking existing code | High | Phase migration, keep Rust fallback during transition |
| Performance regression | Low | Rust primitives unchanged, only loading path differs |
| Circular imports | Medium | `(patina core)` has no dependencies |
| Loader complexity | Medium | Clear priority rules, document well |
| Missing exports | Medium | Comprehensive test suite, compare with chibi |

---

## Test Strategy

### Unit Tests

```rust
#[test]
fn test_patina_core_exports_all_primitives() {
    let interp = TreeWalkInterpreter::new();
    // Verify (patina core) exports +, -, car, cdr, etc.
}

#[test]
fn test_scheme_base_loads_from_sld() {
    let interp = TreeWalkInterpreter::new();
    // Verify (scheme base) loads via .sld file
    // Check that macros (let, cond) are available
}
```

### Integration Tests

```rust
#[test]
fn test_scheme_base_full_api() {
    // Test every R7RS (scheme base) export
}

#[test]
fn test_import_chain() {
    // (scheme base) → (patina core) works
}
```

### Comparison Tests

```bash
# Run same code on chibi and patina, compare results
./scripts/compare_with_chibi.sh
```

---

## Open Questions

1. **Should `(patina core)` be one big library or split?**
   - Option A: Single `(patina core)` with everything
   - Option B: Split: `(patina core)`, `(patina char)`, `(patina io)`, etc.
   - Chibi uses multiple internal libraries, might be cleaner

2. **How to handle special forms?** *(IMPORTANT)*
   - Currently ALL special forms are globally registered - NOT R7RS compliant
   - True special forms (`quote`, `if`, `lambda`, `define`, `set!`, `begin`) must stay in evaluator
   - But they should only be available after importing `(scheme base)` or `(patina core)`
   - `case-lambda` should become a macro (like chibi's SRFI 16), not a special form
   - **Decision needed:** How to make special forms library-scoped?
     - Option A: Check environment for special form binding before applying
     - Option B: Keep global but shadow with `#f` in empty environments
     - Option C: Rethink evaluator architecture

3. **Convert special forms to macros?**
   - `case-lambda` can be a macro (proven by chibi)
   - Reduces evaluator complexity
   - Better library isolation
   - **Decision needed:** Which special forms should become macros?

4. **Loader priority mechanism?**
   - Need clean way to route `(scheme *)` to .sld, `(patina *)` to Rust
   - Could use prefix-based routing

5. **Library visibility: internal vs user-accessible?**

   **How chibi handles this:**
   - `(chibi)` is user-accessible (can import it directly)
   - But most users import `(scheme base)` instead
   - No technical distinction between "internal" and "public"
   - Convention: `(chibi)` is low-level, `(scheme *)` is user-facing

   **Options for Patina:**

   | Option | Structure | Pros | Cons |
   |--------|-----------|------|------|
   | A: All visible | `(patina core)` user-accessible | Simple, like chibi | Users might use internal APIs |
   | B: Naming convention | `(patina %core)` or `(patina internal core)` | Clear intent | Ugly names |
   | C: Loader restriction | Block `(patina core)` from user code | True isolation | Complex loader logic |
   | D: No restriction + docs | `(patina core)` visible, documented as internal | Pragmatic | Relies on docs |

   **Recommendation:** Option D (like chibi) - `(patina core)` is accessible but documented as internal/unstable. User-facing extensions use clear names:
   - `(patina core)` - Internal, unstable API (primitives)
   - `(patina debug)` - User-visible debugging utilities
   - `(patina test)` - User-visible testing framework

6. **REPL behavior?**
   - R7RS doesn't define REPL behavior
   - Should REPL auto-import `(scheme base)` for convenience?
   - Chibi does this: REPL has `(chibi)` available by default

---

## Timeline Estimate

| Phase | Effort | Description |
|-------|--------|-------------|
| Phase 1 | 1-2 hours | Create `(patina core)` |
| Phase 2 | 2-3 hours | Create `lib/scheme/base.sld`, update loaders |
| Phase 3 | 2-3 hours | Migrate other libraries |
| Phase 4 | 1 hour | Cleanup and documentation |
| **Total** | **6-9 hours** | Full migration |

---

## Decision

**Recommendation:** Proceed with migration.

The benefits (R7RS compliance, clarity, maintainability) outweigh the risks. The phased approach allows rollback if issues arise.

**Next Steps:**
1. [ ] Review this document
2. [ ] Decide on open questions
3. [ ] Implement Phase 1
4. [ ] Test and iterate

---

## References

- Chibi-scheme: `lib/scheme/base.sld`
- R7RS §5.6.1: Library declarations
- Patina: `crates/patina-runtime/src/stdlib/`
- Patina: `lib/scheme/base-extras.scm`
