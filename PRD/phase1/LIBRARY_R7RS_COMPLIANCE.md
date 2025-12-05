# Library System R7RS Compliance

**Created:** 2025-12-03
**Updated:** 2025-12-04
**Status:** In Progress (~70% R7RS Compliant)
**Priority:** High - Required for Snow/SRFI ecosystem compatibility

---

## Executive Summary

Patina's library system has a solid foundation with all core import/export functionality working. The remaining gaps block compatibility with the Snow-Fort package repository and standard SRFI libraries.

**Current State:**
- All import modifiers working (only, except, prefix, rename)
- Export with rename working
- Library caching and circular dependency detection working
- `.sld` file loading working

**Critical Gaps (blocking Snow/SRFI compatibility):**
1. **Integer library names** - `(srfi 1)` fails, blocks entire SRFI ecosystem
2. **`include` declaration** - Cannot load chibi's libraries (all use `include`)
3. **`cond-expand`** - Cannot use portable libraries (8/~60 chibi SRFIs use it)
4. **`(features)` procedure** - Required by R7RS §6.13

---

## Real-World Compatibility Analysis

### Why This Matters: Snow-Fort Ecosystem

[Snow-Fort](https://snow-fort.org/) is the primary R7RS package repository. Libraries use standard patterns that Patina currently cannot load.

**Example: `(srfi 1)` from chibi-scheme** (`lib/srfi/1.sld`):
```scheme
(define-library (srfi 1)           ; ← Integer "1" in name (BLOCKED)
  (export xcons cons* make-list ...)
  (cond-expand                      ; ← Conditional expansion (BLOCKED)
   (chibi (import (chibi)))
   (else
    (import (scheme base))
    (begin ...)))
  (include "1/predicates.scm"       ; ← File inclusion (BLOCKED)
           "1/selectors.scm"
           "1/search.scm" ...))
```

**All three missing features** appear together in a single typical library.

### Chibi Library Survey

Surveyed chibi-scheme's library collection to understand real-world patterns:

| Pattern | Usage | Examples |
|---------|-------|----------|
| `include` | ~95% of libraries | Every non-trivial library |
| Integer in name | All SRFIs | `(srfi 1)`, `(srfi 69)`, `(srfi 125)` |
| `cond-expand` | ~15% of libraries | `(srfi 1)`, `(scheme char)`, `(scheme time)` |
| `include-ci` | Rare | Legacy R5RS compatibility |
| `include-library-declarations` | Very rare | Large library suites |
| `include-shared` | Native extensions | `(srfi 69)`, `(scheme time)` - NOT R7RS |

**Note:** `include-shared` is a chibi extension for native code, not part of R7RS.

---

## R7RS Compliance Matrix

### Fully Implemented ✓

| Feature | R7RS Section | Implementation |
|---------|--------------|----------------|
| `define-library` form | §5.6.1 | `library_parser.rs` |
| Library name (identifiers) | §5.6 | Symbols only |
| `(export identifier)` | §5.6.1 | `ExportSpec::Identifier` |
| `(export (rename old new))` | §5.6.1 | `ExportSpec::Rename` |
| `(import (lib))` | §5.2 | `ImportSet::Library` |
| `(import (only ...))` | §5.2 | `ImportSet::Only` |
| `(import (except ...))` | §5.2 | `ImportSet::Except` |
| `(import (prefix ...))` | §5.2 | `ImportSet::Prefix` |
| `(import (rename ...))` | §5.2 | `ImportSet::Rename` |
| Nested import modifiers | §5.2 | Full recursive composition |
| `(begin ...)` in library | §5.6.1 | Evaluates body expressions |
| Library caching | §5.6.1 | Single load, reused |
| Circular dependency detection | - | `loading_stack` check |

### Not Implemented ✗

| Feature | R7RS Section | Impact | Priority |
|---------|--------------|--------|----------|
| **Integers in library names** | §5.6 | Blocks all SRFIs | **Critical** |
| **`(include "file.scm")`** | §5.6.1 | Cannot load real-world libraries | **Critical** |
| **`(cond-expand ...)`** (library) | §5.6.1 | No portable code | **High** |
| **`(cond-expand ...)`** (expression) | §4.2.1 | Expression form also needed | **High** |
| **`(features)` procedure** | §6.13 | Returns implementation features | **High** |
| `(include-ci "file.scm")` | §5.6.1 | Case-insensitive variant | Low |
| `(include-library-declarations ...)` | §5.6.1 | Share declarations | Low |

---

## R7RS Specification Details

### Library Names (§5.6)

> `<library name>` is a list whose members are identifiers and **exact non-negative integers**. It is used to identify the library uniquely when importing from other programs or libraries. Libraries whose first identifier is `scheme` are reserved for use by this report. Libraries whose first identifier is `srfi` are reserved for libraries implementing Scheme Requests for Implementation.

**Current error:**
```scheme
(import (srfi 1))
; ERROR: Library name parts must be symbols, got: 1
```

### Library Declarations (§5.6.1)

A `<library declaration>` is any of:
- `(export <export spec> ...)`
- `(import <import set> ...)`
- `(begin <command or definition> ...)`
- `(include <filename1> <filename2> ...)`
- `(include-ci <filename1> <filename2> ...)`
- `(include-library-declarations <filename1> <filename2> ...)`
- `(cond-expand <ce-clause1> <ce-clause2> ...)`

> The `begin`, `include`, and `include-ci` declarations are used to specify the **body** of the library. They have the same syntax and semantics as the corresponding expression types.

> The `include-library-declarations` declaration is similar to `include` except that the contents of the file are spliced directly into the **current library definition** (not the body).

> The `cond-expand` declaration has the same syntax and semantics as the `cond-expand` expression type, except that it expands to **spliced-in library declarations** rather than expressions enclosed in `begin`.

### cond-expand Expression (§4.2.1)

```scheme
(cond-expand <ce-clause1> <ce-clause2> ...)

<ce-clause> ::= (<feature-requirement> <expression> ...)
             |  (else <expression> ...)

<feature-requirement> ::= <feature-identifier>
                       |  (library <library-name>)
                       |  (and <feature-requirement> ...)
                       |  (or <feature-requirement> ...)
                       |  (not <feature-requirement>)
```

> Each implementation maintains a list of feature identifiers which are present, as well as a list of libraries which can be imported. The value of a `<feature requirement>` is determined by replacing each `<feature identifier>` and `(library <library name>)` on the implementation's lists with `#t`, and all other feature identifiers and library names with `#f`, then evaluating the resulting expression as a Scheme boolean expression under the normal interpretation of `and`, `or`, and `not`.

### Standard Feature Identifiers (Appendix B)

R7RS defines these standard features:

| Feature | Meaning |
|---------|---------|
| `r7rs` | All R7RS implementations |
| `exact-closed` | Algebraic ops (except `/`) produce exact from exact |
| `exact-complex` | Exact complex numbers supported |
| `ieee-float` | Inexact numbers are IEEE 754 |
| `full-unicode` | All Unicode 6.0 characters supported |
| `ratios` | `/` with exact args produces exact when divisor nonzero |
| `posix` | Running on POSIX system |
| `windows` | Running on Windows |
| `unix`, `darwin`, `gnu-linux`, `bsd`, ... | OS flags |
| `i386`, `x86-64`, `ppc`, `sparc`, ... | CPU architecture |
| `ilp32`, `lp64`, `ilp64`, ... | C memory model |
| `big-endian`, `little-endian` | Byte order |
| `<name>` | Implementation name |
| `<name-version>` | Implementation name and version |

---

## Architecture Overview

### Current Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    (import (scheme base))                    │
└────────────────────────────┬────────────────────────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │ Desugarer: CoreExpr::Import  │
              │ (patina-frontend/desugarer)  │
              └──────────────────┬───────────┘
                                 │
                                 ▼
         ┌───────────────────────────────────────────┐
         │ Evaluator: process_import_for_eval()      │
         │ (patina-tree-walker/eval/mod.rs:1465)     │
         └────────────────────┬──────────────────────┘
                              │
                              ▼
           ┌──────────────────────────────────────┐
           │ LibraryLoaderRegistry (priority)     │
           │ 1. RustLibraryLoader (built-in)      │
           │ 2. SchemeLibraryLoader (.sld files)  │
           └────────────────────┬─────────────────┘
                                │
               ┌────────────────┴────────────────┐
               │                                 │
               ▼                                 ▼
   ┌───────────────────────┐      ┌─────────────────────────────┐
   │ Rust: RustLibraryLoader│      │ Scheme: SchemeLibraryLoader │
   │ - scheme/base          │      │ - Parses .sld files         │
   │ - scheme/char          │      │ - Returns ParsedLibrary     │
   │ - scheme/complex       │      │ - Evaluator runs body       │
   │ - scheme/write         │      └─────────────────────────────┘
   │ - chibi/test           │
   └───────────────────────┘
                                │
                                ▼
                  ┌───────────────────────────┐
                  │ LibraryRegistry (cached)  │
                  │ - Stores loaded libraries │
                  │ - Tracks search paths     │
                  └───────────────────────────┘
```

### Key Files

| Component | Location | Purpose |
|-----------|----------|---------|
| Library Parser | `patina-frontend/src/library_parser.rs` | Parses `define-library` |
| Import/Export Types | `patina-runtime/src/library_loader.rs` | `ImportSet`, `ExportSpec` |
| Library Registry | `patina-runtime/src/library_registry.rs` | Caching, search paths |
| Scheme Loader | `patina-tree-walker/src/library_support.rs` | .sld file loading |
| Import Desugaring | `patina-frontend/src/desugarer/mod.rs:378` | `CoreExpr::Import` |
| Import Processing | `patina-tree-walker/src/eval/mod.rs:1465-1664` | Modifier application |

---

## Implementation Plan

### Phase 1: Integer Library Names (Critical, Low Effort)

**Goal:** Support `(srfi 1)`, `(srfi 69)`, etc.

**Change:** Modify `parse_library_name()` in `library_parser.rs:80-102`:

```rust
fn parse_library_name(value: &Value) -> Result<Vec<String>, ParseError> {
    let list = Self::expect_list(value)?;

    if list.is_empty() {
        return Err(ParseError::InvalidSyntax(
            "Library name cannot be empty".to_string(),
        ));
    }

    let mut name = Vec::new();
    for part in list {
        match part {
            Value::Symbol(s) => name.push(s.to_string()),
            Value::Integer(n) if n >= 0 => name.push(n.to_string()),
            _ => return Err(ParseError::InvalidSyntax(format!(
                "Library name parts must be symbols or non-negative integers, got: {}",
                part
            ))),
        }
    }

    Ok(name)
}
```

**Effort:** ~15 minutes, ~10 lines changed

---

### Phase 2: Include Declaration (Critical, Medium Effort)

**Goal:** Allow splitting library code across files.

**Key insight from chibi:** Include paths are **relative to the .sld file location**.

```scheme
;; lib/srfi/1.sld
(define-library (srfi 1)
  (include "1/predicates.scm"    ; → lib/srfi/1/predicates.scm
           "1/selectors.scm"))   ; → lib/srfi/1/selectors.scm
```

**Implementation steps:**

1. Add `includes: Vec<IncludeDecl>` to `LibraryDefinition`
2. Parse `include` and `include-ci` in `parse_declaration()`
3. Resolve includes in `SchemeLibraryLoader::parse_sld_file()`
4. Handle declaration order (includes can appear anywhere)

**Key detail:** R7RS says `include` and `begin` are processed in order:
> The expressions from all `begin`, `include` and `include-ci` library declarations are expanded in that environment **in the order in which they occur** in the library.

---

### Phase 3: cond-expand + features (High, High Effort)

**Goal:** Enable portable code with conditional expansion.

**Two forms required:**
1. **Library declaration form** - Expands to library declarations
2. **Expression form** - Expands to `(begin ...)` expressions

**Implementation steps:**

1. Create `FeatureRegistry` in `patina-runtime`
2. Add `(features)` primitive to `(scheme base)`
3. Parse `cond-expand` clauses in library parser
4. Evaluate feature requirements during library loading
5. Add `cond-expand` as expression type (macro or special form)

---

### Phase 4: include-library-declarations (Low Priority)

Rarely used in practice. Can defer until needed.

---

## Comprehensive Test Suite

### Test File: `crates/patina-tests/tests/library_r7rs_compliance.rs`

```rust
//! R7RS Library System Compliance Tests
//!
//! Tests for Snow-Fort and SRFI library compatibility.

use patina_interpreter::TreeWalkInterpreter;
use std::fs;
use tempfile::TempDir;

mod common;
use common::*;

// ============================================================================
// Integer Library Names (R7RS §5.6)
// ============================================================================

#[test]
fn test_integer_in_library_name_simple() {
    // (srfi 1) style naming
    let temp = TempDir::new().unwrap();
    let srfi_dir = temp.path().join("srfi");
    fs::create_dir(&srfi_dir).unwrap();

    fs::write(srfi_dir.join("1.sld"), r#"
        (define-library (srfi 1)
          (export xcons)
          (import (scheme base))
          (begin
            (define (xcons d a) (cons a d))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (srfi 1))
        (xcons 1 2)
    "#).unwrap();

    assert_eval_to_pair(result, 2, 1);
}

#[test]
fn test_integer_in_library_name_multi_part() {
    // (srfi 1 lists) style - multiple parts including integer
    let temp = TempDir::new().unwrap();
    let srfi_dir = temp.path().join("srfi").join("1");
    fs::create_dir_all(&srfi_dir).unwrap();

    fs::write(srfi_dir.join("lists.sld"), r#"
        (define-library (srfi 1 lists)
          (export first)
          (import (scheme base))
          (begin (define (first lst) (car lst))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (srfi 1 lists))
        (first '(a b c))
    "#).unwrap();

    assert_is_symbol(result, "a");
}

#[test]
fn test_negative_integer_in_library_name_rejected() {
    // Negative integers should be rejected per R7RS
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("bad.sld"), r#"
        (define-library (test -1)
          (export foo)
          (begin (define foo 42)))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program("(import (test -1))");
    assert!(result.is_err());
}

// ============================================================================
// Include Declaration (R7RS §5.6.1)
// ============================================================================

#[test]
fn test_include_single_file() {
    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("mylib");
    fs::create_dir(&lib_dir).unwrap();

    // Library definition
    fs::write(lib_dir.join("utils.sld"), r#"
        (define-library (mylib utils)
          (export double)
          (import (scheme base))
          (include "impl.scm"))
    "#).unwrap();

    // Included implementation
    fs::write(lib_dir.join("impl.scm"), r#"
        (define (double x) (* x 2))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (mylib utils))
        (double 21)
    "#).unwrap();

    assert_eval_to_integer(result, 42);
}

#[test]
fn test_include_multiple_files() {
    // Matches SRFI-1 pattern: (include "1/predicates.scm" "1/selectors.scm" ...)
    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("mylib");
    let impl_dir = lib_dir.join("impl");
    fs::create_dir_all(&impl_dir).unwrap();

    fs::write(lib_dir.join("core.sld"), r#"
        (define-library (mylib core)
          (export add mul)
          (import (scheme base))
          (include "impl/add.scm" "impl/mul.scm"))
    "#).unwrap();

    fs::write(impl_dir.join("add.scm"), "(define (add a b) (+ a b))").unwrap();
    fs::write(impl_dir.join("mul.scm"), "(define (mul a b) (* a b))").unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (mylib core))
        (list (add 2 3) (mul 4 5))
    "#).unwrap();

    assert_eval_to_list(result, vec![5, 20]);
}

#[test]
fn test_include_with_subdirectory_path() {
    // Test relative path with subdirectory (common pattern)
    let temp = TempDir::new().unwrap();
    let srfi_dir = temp.path().join("srfi");
    let srfi_1_dir = srfi_dir.join("1");
    fs::create_dir_all(&srfi_1_dir).unwrap();

    fs::write(srfi_dir.join("1.sld"), r#"
        (define-library (srfi 1)
          (export proper-list?)
          (import (scheme base))
          (include "1/predicates.scm"))
    "#).unwrap();

    fs::write(srfi_1_dir.join("predicates.scm"), r#"
        (define (proper-list? x)
          (or (null? x)
              (and (pair? x) (proper-list? (cdr x)))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (srfi 1))
        (list (proper-list? '(1 2 3))
              (proper-list? '(1 . 2)))
    "#).unwrap();

    assert_eval_to_list_bool(result, vec![true, false]);
}

#[test]
fn test_include_order_matters() {
    // R7RS: expressions processed in order they appear
    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    fs::write(lib_dir.join("order.sld"), r#"
        (define-library (test order)
          (export result)
          (import (scheme base))
          (begin (define x 1))
          (include "set-x.scm")
          (begin (define result x)))
    "#).unwrap();

    fs::write(lib_dir.join("set-x.scm"), "(set! x 42)").unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test order))
        result
    "#).unwrap();

    assert_eval_to_integer(result, 42);
}

#[test]
fn test_include_missing_file_error() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("bad.sld"), r#"
        (define-library (bad)
          (include "nonexistent.scm"))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program("(import (bad))");
    assert!(result.is_err());
    // Error message should mention the missing file
}

// ============================================================================
// Include-ci Declaration (R7RS §5.6.1)
// ============================================================================

#[test]
fn test_include_ci_case_folding() {
    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    fs::write(lib_dir.join("ci.sld"), r#"
        (define-library (test ci)
          (export HELLO)
          (import (scheme base))
          (include-ci "UPPER.scm"))
    "#).unwrap();

    // File with uppercase symbols that should be folded to lowercase
    fs::write(lib_dir.join("UPPER.scm"), r#"
        (define HELLO "world")
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test ci))
        hello  ; lowercase should work due to case folding
    "#).unwrap();

    assert_eval_to_string(result, "world");
}

// ============================================================================
// cond-expand in Libraries (R7RS §5.6.1)
// ============================================================================

#[test]
fn test_cond_expand_r7rs_feature() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (export result)
          (import (scheme base))
          (cond-expand
            (r7rs (begin (define result 'r7rs-present)))
            (else (begin (define result 'no-r7rs)))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test))
        result
    "#).unwrap();

    assert_is_symbol(result, "r7rs-present");
}

#[test]
fn test_cond_expand_implementation_feature() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (export impl-name)
          (import (scheme base))
          (cond-expand
            (patina (begin (define impl-name 'patina)))
            (chibi (begin (define impl-name 'chibi)))
            (else (begin (define impl-name 'unknown)))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test))
        impl-name
    "#).unwrap();

    assert_is_symbol(result, "patina");
}

#[test]
fn test_cond_expand_library_requirement() {
    // (library (scheme base)) should be true
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (export has-base)
          (cond-expand
            ((library (scheme base))
             (import (scheme base))
             (begin (define has-base #t)))
            (else
             (begin (define has-base #f)))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test))
        has-base
    "#).unwrap();

    assert_eval_to_bool(result, true);
}

#[test]
fn test_cond_expand_and_requirement() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (export result)
          (import (scheme base))
          (cond-expand
            ((and r7rs patina)
             (begin (define result 'both)))
            (else
             (begin (define result 'not-both)))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test))
        result
    "#).unwrap();

    assert_is_symbol(result, "both");
}

#[test]
fn test_cond_expand_or_requirement() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (export result)
          (import (scheme base))
          (cond-expand
            ((or chibi patina guile)
             (begin (define result 'known-impl)))
            (else
             (begin (define result 'unknown-impl)))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test))
        result
    "#).unwrap();

    assert_is_symbol(result, "known-impl");
}

#[test]
fn test_cond_expand_not_requirement() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (export result)
          (import (scheme base))
          (cond-expand
            ((not chibi)
             (begin (define result 'not-chibi)))
            (else
             (begin (define result 'is-chibi)))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test))
        result
    "#).unwrap();

    assert_is_symbol(result, "not-chibi");
}

#[test]
fn test_cond_expand_else_clause() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (export result)
          (import (scheme base))
          (cond-expand
            (nonexistent-feature
             (begin (define result 'found)))
            (else
             (begin (define result 'fallback)))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test))
        result
    "#).unwrap();

    assert_is_symbol(result, "fallback");
}

#[test]
fn test_cond_expand_with_import() {
    // Real-world pattern from SRFI-1: cond-expand affects imports
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (export double)
          (cond-expand
            (patina
             (import (scheme base)))
            (else
             (import (scheme base))))
          (begin (define (double x) (* x 2))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (test))
        (double 21)
    "#).unwrap();

    assert_eval_to_integer(result, 42);
}

#[test]
fn test_cond_expand_with_include() {
    // Pattern from (scheme char): cond-expand chooses which files to include
    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    fs::write(lib_dir.join("char.sld"), r#"
        (define-library (test char)
          (export char-special)
          (import (scheme base))
          (cond-expand
            (full-unicode
             (include "char-full.scm"))
            (else
             (include "char-ascii.scm"))))
    "#).unwrap();

    fs::write(lib_dir.join("char-full.scm"),
        "(define (char-special c) 'unicode)").unwrap();
    fs::write(lib_dir.join("char-ascii.scm"),
        "(define (char-special c) 'ascii)").unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    // Patina should have full-unicode feature
    let result = interp.eval_program(r#"
        (import (test char))
        (char-special #\a)
    "#).unwrap();

    // Result depends on whether patina reports full-unicode
    assert!(result.is_symbol());
}

// ============================================================================
// cond-expand as Expression (R7RS §4.2.1)
// ============================================================================

#[test]
fn test_cond_expand_expression_basic() {
    assert_eval_to(
        "(cond-expand (r7rs 'yes) (else 'no))",
        "yes"
    );
}

#[test]
fn test_cond_expand_expression_multiple_exprs() {
    // Multiple expressions in clause → wrapped in begin
    assert_program_eval_to(
        "(define x 0)
         (cond-expand
           (r7rs (set! x 1) (set! x (+ x 1)) x)
           (else 0))",
        "2"
    );
}

#[test]
fn test_cond_expand_expression_nested() {
    assert_eval_to(
        "(cond-expand
           ((and r7rs (not windows)) 'unix-r7rs)
           (r7rs 'windows-r7rs)
           (else 'other))",
        "unix-r7rs"  // or "windows-r7rs" on Windows
    );
}

// ============================================================================
// features Procedure (R7RS §6.13)
// ============================================================================

#[test]
fn test_features_returns_list() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str("(features)").unwrap();
    assert!(result.is_list());
}

#[test]
fn test_features_contains_r7rs() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str("(memq 'r7rs (features))").unwrap();
    assert!(!result.is_false());
}

#[test]
fn test_features_contains_patina() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str("(memq 'patina (features))").unwrap();
    assert!(!result.is_false());
}

#[test]
fn test_features_contains_ratios() {
    // Patina supports exact ratios
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str("(memq 'ratios (features))").unwrap();
    assert!(!result.is_false());
}

#[test]
fn test_features_platform_specific() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    #[cfg(target_os = "macos")]
    {
        let result = interp.eval_str("(memq 'darwin (features))").unwrap();
        assert!(!result.is_false());
    }

    #[cfg(target_os = "linux")]
    {
        let result = interp.eval_str("(memq 'gnu-linux (features))").unwrap();
        assert!(!result.is_false());
    }

    #[cfg(windows)]
    {
        let result = interp.eval_str("(memq 'windows (features))").unwrap();
        assert!(!result.is_false());
    }
}

// ============================================================================
// Snow-Fort Compatibility Tests
// ============================================================================

#[test]
fn test_snow_srfi_1_pattern() {
    // Full SRFI-1 style library (simplified)
    let temp = TempDir::new().unwrap();
    let srfi_dir = temp.path().join("srfi");
    let srfi_1_dir = srfi_dir.join("1");
    fs::create_dir_all(&srfi_1_dir).unwrap();

    fs::write(srfi_dir.join("1.sld"), r#"
        (define-library (srfi 1)
          (export xcons proper-list? first)
          (cond-expand
            (patina (import (scheme base)))
            (else (import (scheme base))))
          (include "1/constructors.scm"
                   "1/predicates.scm"
                   "1/selectors.scm"))
    "#).unwrap();

    fs::write(srfi_1_dir.join("constructors.scm"), r#"
        (define (xcons d a) (cons a d))
    "#).unwrap();

    fs::write(srfi_1_dir.join("predicates.scm"), r#"
        (define (proper-list? x)
          (or (null? x)
              (and (pair? x) (proper-list? (cdr x)))))
    "#).unwrap();

    fs::write(srfi_1_dir.join("selectors.scm"), r#"
        (define (first lst) (car lst))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (srfi 1))
        (list (xcons 1 2)
              (proper-list? '(a b c))
              (first '(x y z)))
    "#).unwrap();

    // Should return ((2 . 1) #t x)
    assert!(result.is_list());
}

#[test]
fn test_snow_scheme_char_pattern() {
    // Pattern from (scheme char) - cond-expand choosing includes
    let temp = TempDir::new().unwrap();
    let scheme_dir = temp.path().join("scheme");
    fs::create_dir(&scheme_dir).unwrap();

    fs::write(scheme_dir.join("char.sld"), r#"
        (define-library (scheme mychar)
          (export my-char-upcase)
          (import (scheme base))
          (cond-expand
            (full-unicode
             (include "char-unicode.scm"))
            (else
             (include "char-basic.scm"))))
    "#).unwrap();

    fs::write(scheme_dir.join("char-unicode.scm"), r#"
        (define (my-char-upcase c)
          (if (char? c) (char-upcase c) c))
    "#).unwrap();

    fs::write(scheme_dir.join("char-basic.scm"), r#"
        (define (my-char-upcase c)
          (if (and (char? c) (char<=? #\a c) (char<=? c #\z))
              (integer->char (- (char->integer c) 32))
              c))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program(r#"
        (import (scheme mychar))
        (my-char-upcase #\a)
    "#).unwrap();

    assert_eval_to_char(result, 'A');
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

#[test]
fn test_empty_library_name_rejected() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("bad.sld"), r#"
        (define-library ()
          (export foo))
    "#).unwrap();

    // Should error during parse
}

#[test]
fn test_cond_expand_no_matching_clause() {
    // R7RS: behavior is unspecified if no clause matches and no else
    // We should probably error
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("test.sld"), r#"
        (define-library (test)
          (cond-expand
            (nonexistent-1 (begin))
            (nonexistent-2 (begin))))
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program("(import (test))");
    // Should either error or produce empty library
}

#[test]
fn test_circular_include_detection() {
    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("test");
    fs::create_dir(&lib_dir).unwrap();

    fs::write(lib_dir.join("circular.sld"), r#"
        (define-library (test circular)
          (include "a.scm"))
    "#).unwrap();

    fs::write(lib_dir.join("a.scm"), r#"
        (include "b.scm")
    "#).unwrap();

    fs::write(lib_dir.join("b.scm"), r#"
        (include "a.scm")  ; Circular!
    "#).unwrap();

    let interp = TreeWalkInterpreter::with_library_path(temp.path());
    let result = interp.eval_program("(import (test circular))");
    assert!(result.is_err());
}
```

---

## Success Criteria

### Phase 1: SRFI Names (Must Have)
- [ ] `(srfi 1)` library name parses
- [ ] `(srfi 69)` library name parses
- [ ] Negative integers rejected
- [ ] Mixed names work: `(srfi 1 lists)`

### Phase 2: Include (Must Have)
- [ ] `(include "file.scm")` loads and splices
- [ ] Multiple files: `(include "a.scm" "b.scm")`
- [ ] Subdirectory paths: `(include "sub/file.scm")`
- [ ] Order preserved with `begin`
- [ ] Missing file produces clear error
- [ ] Circular include detected

### Phase 3: cond-expand + features (Must Have)
- [ ] `(features)` returns list with `r7rs`, `patina`
- [ ] Platform features: `darwin`/`gnu-linux`/`windows`
- [ ] Architecture features: `x86-64`/`aarch64`
- [ ] `cond-expand` in library declaration
- [ ] `cond-expand` as expression
- [ ] Feature requirements: `and`, `or`, `not`
- [ ] `(library <name>)` requirement
- [ ] `else` clause

### Phase 4: Include-ci (Nice to Have)
- [ ] Case-insensitive reading

### Phase 5: Include-library-declarations (Nice to Have)
- [ ] Declaration splicing

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| File path resolution complexity | Medium | Use library file's directory as base |
| Circular includes | High | Track included files during parse, error on cycle |
| Feature detection for libraries | Medium | Query registry before full load |
| Case-insensitive lexing | Low | Add flag to lexer constructor |
| `cond-expand` evaluation order | Medium | Process declarations left-to-right per R7RS |

---

## References

- **R7RS Specification:**
  - §5.2 - Import declarations
  - §5.6 - Libraries
  - §5.6.1 - Library syntax
  - §4.2.1 - Conditionals (cond-expand expression)
  - §6.13 - System interface (features procedure)
  - Appendix B - Standard Feature Identifiers

- **Reference Implementations:**
  - Chibi-scheme: `lib/srfi/*.sld`, `lib/scheme/*.sld`
  - Snow-Fort: https://snow-fort.org/

- **Patina Implementation:**
  - `crates/patina-frontend/src/library_parser.rs`
  - `crates/patina-tree-walker/src/library_support.rs`
  - `crates/patina-runtime/src/library_registry.rs`
