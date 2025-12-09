# Library System R7RS Compliance

**Created:** 2025-12-03
**Updated:** 2025-12-08
**Status:** Complete (100% R7RS Compliant)
**Priority:** High - Required for Snow/SRFI ecosystem compatibility

---

## Executive Summary

Patina's library system has a solid foundation with all core import/export functionality working. The remaining gaps block compatibility with the Snow-Fort package repository and standard SRFI libraries.

**Current State:**
- ✅ All import modifiers working (only, except, prefix, rename)
- ✅ Export with rename working
- ✅ Library caching and circular dependency detection working
- ✅ Integer library names working - `(srfi 1)`, `(srfi 69)`, etc. *(completed 2025-12-05)*
- ✅ `(include "file.scm")` declaration working *(completed 2025-12-05)*
- ✅ Full `.sld` file loading working (with includes and cond-expand!)

**What ".sld file loading" supports:**
- ✅ File discovery via search paths (`./lib`, `$PATINA_HOME/lib`, workspace root)
- ✅ Parsing `define-library` forms with export/import/begin
- ✅ Recursive library imports and caching
- ✅ `(include "file.scm")` - resolves relative to .sld file, multiple files, subdirs
- ✅ `(include-ci "file.scm")` - case-insensitive reading *(completed 2025-12-08)*
- ✅ `(include-library-declarations "file.scm")` - declaration splicing *(completed 2025-12-08)*
- ✅ `(cond-expand ...)` - feature-based conditional expansion *(completed 2025-12-05)*
- ✅ `(features)` procedure - returns list of supported features *(completed 2025-12-05)*

**Remaining Gaps:** ✅ ALL COMPLETE
1. ~~**Integer library names** - `(srfi 1)` fails, blocks entire SRFI ecosystem~~ ✅ DONE
2. ~~**`include` declaration** - Cannot load chibi's libraries (all use `include`)~~ ✅ DONE
3. ~~**`cond-expand`** - Cannot use portable libraries (8/~60 chibi SRFIs use it)~~ ✅ DONE
4. ~~**`(features)` procedure** - Required by R7RS §6.13~~ ✅ DONE
5. ~~`(include-ci "file.scm")` - case-insensitive reading~~ ✅ DONE (2025-12-08)
6. ~~`(include-library-declarations ...)` - declaration splicing~~ ✅ DONE (2025-12-08)

---

## Real-World Compatibility Analysis

### Why This Matters: Snow-Fort Ecosystem

[Snow-Fort](https://snow-fort.org/) is the primary R7RS package repository. Libraries use standard patterns that Patina currently cannot load.

**Example: `(srfi 1)` from chibi-scheme** (`lib/srfi/1.sld`):
```scheme
(define-library (srfi 1)           ; ← Integer "1" in name ✅ WORKS
  (export xcons cons* make-list ...)
  (cond-expand                      ; ← Conditional expansion (BLOCKED)
   (chibi (import (chibi)))
   (else
    (import (scheme base))
    (begin ...)))
  (include "1/predicates.scm"       ; ← File inclusion ✅ WORKS
           "1/selectors.scm"
           "1/search.scm" ...))
```

**One remaining blocker** (`cond-expand`) prevents loading some portable libraries.

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
| Library name (identifiers) | §5.6 | Symbols in name |
| Library name (integers) | §5.6 | Non-negative integers *(2025-12-05)* |
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
| `.sld` file discovery | §5.6 | Multi-path search |
| `(include "file.scm" ...)` | §5.6.1 | *(2025-12-05)* Relative to .sld, multiple files |
| `(include-ci "file.scm" ...)` | §5.6.1 | *(2025-12-05)* Parsed, reader mode deferred |

### Implemented ✓ (continued)

| Feature | R7RS Section | Implementation |
|---------|--------------|----------------|
| `(cond-expand ...)` (library) | §5.6.1 | `library_parser.rs:parse_cond_expand` *(2025-12-05)* |
| `(cond-expand ...)` (expression) | §4.2.1 | `desugarer/mod.rs:desugar_cond_expand` *(2025-12-05)* |
| `(features)` procedure | §6.13 | `primitives/system.rs` *(2025-12-05)* |
| Feature requirements | §4.2.1 | `cond_expand.rs:evaluate_feature_requirement` *(2025-12-05)* |

### Fully Implemented ✓ (continued 2)

| Feature | R7RS Section | Implementation |
|---------|--------------|----------------|
| `(include-ci "file.scm")` | §5.6.1 | `library_support.rs` + case-insensitive lexer *(2025-12-08)* |
| `(include-library-declarations ...)` | §5.6.1 | `library_support.rs:parse_library_declarations_file` *(2025-12-08)* |

### Fully Implemented ✓ (continued 3)

| Feature | R7RS Section | Implementation |
|---------|--------------|----------------|
| `(library <name>)` in cond-expand | §4.2.1 | `library_loader.rs:can_load_with_paths` *(2025-12-08)* |

*All R7RS library system features are now fully implemented!*

---

## R7RS Specification Details

### Library Names (§5.6)

> `<library name>` is a list whose members are identifiers and **exact non-negative integers**. It is used to identify the library uniquely when importing from other programs or libraries. Libraries whose first identifier is `scheme` are reserved for use by this report. Libraries whose first identifier is `srfi` are reserved for libraries implementing Scheme Requests for Implementation.

**✅ Implemented (2025-12-05):** Integer library names now work:
```scheme
(import (srfi 1))     ; Works!
(import (srfi 69))    ; Works!
(import (lib 1 2 3))  ; Works!
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

### Phase 1: Integer Library Names ✅ COMPLETE

**Goal:** Support `(srfi 1)`, `(srfi 69)`, etc.

**Status:** ✅ Completed 2025-12-05

**Changes made:**
- Modified `parse_library_name()` in `library_parser.rs:83-106`
- Accepts `Value::Integer(n)` where `n >= 0`, converts to string
- Added 9 unit tests covering all cases

---

### Phase 2: Include Declaration ✅ COMPLETE

**Goal:** Allow splitting library code across files.

**Status:** ✅ Completed 2025-12-05

**Changes made:**
- Added `BodyElement` enum to `library_parser.rs` to preserve declaration order
- Updated `LibraryDefinition` to use `body_elements: Vec<BodyElement>`
- Added `resolve_body_elements()` to `SchemeLibraryLoader` in `library_support.rs`
- Added `parse_all()` method to Parser for parsing multiple expressions
- Added 8 unit tests for include parsing
- Added 4 integration tests for include loading

**Key insight from chibi:** Include paths are **relative to the .sld file location**.

```scheme
;; lib/srfi/1.sld
(define-library (srfi 1)
  (include "1/predicates.scm"    ; → lib/srfi/1/predicates.scm
           "1/selectors.scm"))   ; → lib/srfi/1/selectors.scm
```

#### Architecture Decision: Where to Resolve Includes

**Option A: Resolve during parsing (in library_parser.rs)** ❌
- Problem: Parser doesn't know the .sld file's path
- Would need to pass file path through parser

**Option B: Resolve during library loading (in library_support.rs)** ✅ RECOMMENDED
- `SchemeLibraryLoader::parse_sld_file()` already has the file path
- Can read included files and splice into body
- Keeps parser simple and stateless

#### Data Model Changes

**File:** `patina-frontend/src/library_parser.rs`

```rust
/// Represents a body element in declaration order
#[derive(Debug, Clone)]
pub enum BodyElement {
    /// Inline code from (begin ...)
    Begin(Vec<Value>),
    /// File to include: (include "file.scm" ...)
    Include { paths: Vec<String>, case_insensitive: bool },
}

pub struct LibraryDefinition {
    pub name: Vec<String>,
    pub exports: Vec<ExportSpec>,
    pub imports: Vec<ImportSet>,
    pub body_elements: Vec<BodyElement>,  // Replaces `body: Vec<Value>`
}
```

#### Implementation Steps

**Step 1: Update LibraryDefinition structure**
- Add `BodyElement` enum
- Change `body: Vec<Value>` to `body_elements: Vec<BodyElement>`
- Update `parse_declaration()` to handle `include` and `include-ci`

**Step 2: Update SchemeLibraryLoader**
Location: `patina-tree-walker/src/library_support.rs`

```rust
impl SchemeLibraryLoader {
    fn resolve_body_elements(
        &self,
        elements: &[BodyElement],
        sld_dir: &Path,
        included_files: &mut HashSet<PathBuf>,  // For cycle detection
    ) -> Result<Vec<Value>, LibraryError> {
        let mut body = Vec::new();

        for element in elements {
            match element {
                BodyElement::Begin(exprs) => {
                    body.extend(exprs.clone());
                }
                BodyElement::Include { paths, case_insensitive } => {
                    for path in paths {
                        let file_path = sld_dir.join(path);
                        let canonical = file_path.canonicalize()?;

                        // Cycle detection
                        if !included_files.insert(canonical.clone()) {
                            return Err(LibraryError::CircularInclude(canonical));
                        }

                        // Read and parse file
                        let content = std::fs::read_to_string(&file_path)?;
                        let exprs = self.parse_included_file(&content, *case_insensitive)?;
                        body.extend(exprs);
                    }
                }
            }
        }

        Ok(body)
    }

    fn parse_included_file(&self, content: &str, case_insensitive: bool) -> Result<Vec<Value>, LibraryError> {
        let mut parser = if case_insensitive {
            Parser::new_case_insensitive(content)  // Need to add this
        } else {
            Parser::new(content)
        };

        let mut exprs = Vec::new();
        while let Some(expr) = parser.parse_expr()? {
            exprs.push(expr);
        }
        Ok(exprs)
    }
}
```

**Step 3: Update ParsedLibrary construction**
- Call `resolve_body_elements()` in `parse_sld_file()`
- Pass resolved body to evaluator

**Step 4: Add case-insensitive lexer mode (for include-ci)**
- Add `case_insensitive: bool` flag to Lexer
- When true, fold symbol names to lowercase
- Low priority - can defer if not needed immediately

#### Error Handling

| Error Case | Message |
|------------|---------|
| File not found | `Include file not found: {path} (from {sld_file})` |
| Circular include | `Circular include detected: {file} already included` |
| Parse error in included file | `Error parsing {file}: {error}` |
| Path traversal attempt | `Include path cannot traverse above library directory` |

---

### Phase 3: cond-expand + features ✅ COMPLETE

**Goal:** Enable portable code with conditional expansion.

**Status:** ✅ Completed 2025-12-05

**Changes made:**
- Created `FeatureRegistry` in `patina-runtime/src/features.rs`
  - Detects platform: darwin, gnu-linux, windows, posix, unix
  - Detects architecture: x86-64, aarch64
  - Detects endianness: little-endian, big-endian
  - Reports capabilities: r7rs, patina, ratios, exact-closed, ieee-float, full-unicode
- Added `(features)` primitive to `(scheme base)` in `primitives/system.rs`
- Created `evaluate_feature_requirement()` in `patina-frontend/src/cond_expand.rs`
  - Handles: and, or, not, library requirements
- Added `parse_cond_expand()` in `library_parser.rs` for library declarations
- Added `desugar_cond_expand()` in `desugarer/mod.rs` for expressions
- Added 28 unit tests across cond_expand.rs, library_parser.rs, and desugarer/mod.rs

**Note:** The `(library <name>)` requirement currently returns false because the desugarer doesn't have access to the library loader registry. This could be enhanced later.

#### Two Forms Required

1. **Library declaration form** - Expands to spliced library declarations
2. **Expression form** - Expands to `(begin ...)` expressions

Both use the same feature requirement evaluation logic.

#### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     FeatureRegistry                          │
│  (patina-runtime/src/features.rs)                           │
│                                                              │
│  Static features:     Dynamic features:                      │
│  - r7rs               - (library X) queries                  │
│  - patina             - Runtime platform detection           │
│  - ratios                                                    │
│  - exact-closed                                              │
│  - ieee-float                                                │
│  - full-unicode                                              │
│  - darwin/gnu-linux/windows (compile-time)                   │
│  - x86-64/aarch64 (compile-time)                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│            Feature Requirement Evaluator                     │
│                                                              │
│  Input: (<feature-requirement>)                              │
│  Output: bool                                                │
│                                                              │
│  Patterns:                                                   │
│  - <identifier>         → features.contains(id)              │
│  - (library <name>)     → library_registry.can_load(name)    │
│  - (and req1 req2 ...)  → all requirements true              │
│  - (or req1 req2 ...)   → any requirement true               │
│  - (not req)            → requirement false                  │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
┌─────────────────────────┐     ┌─────────────────────────────┐
│  Library cond-expand    │     │  Expression cond-expand     │
│  (library_parser.rs)    │     │  (macro in base-extras.scm  │
│                         │     │   or special form)          │
│  Splices declarations:  │     │                             │
│  - export               │     │  Expands to (begin ...)     │
│  - import               │     │                             │
│  - begin                │     │                             │
│  - include              │     │                             │
└─────────────────────────┘     └─────────────────────────────┘
```

#### Implementation Steps

**Step 1: Create FeatureRegistry**

File: `patina-runtime/src/features.rs`

```rust
use std::collections::HashSet;

pub struct FeatureRegistry {
    features: HashSet<String>,
}

impl FeatureRegistry {
    pub fn new() -> Self {
        let mut features = HashSet::new();

        // R7RS required
        features.insert("r7rs".to_string());

        // Implementation
        features.insert("patina".to_string());

        // Numeric capabilities
        features.insert("ratios".to_string());
        features.insert("exact-closed".to_string());
        features.insert("ieee-float".to_string());

        // Unicode
        features.insert("full-unicode".to_string());

        // Platform (compile-time)
        #[cfg(target_os = "macos")]
        {
            features.insert("darwin".to_string());
            features.insert("posix".to_string());
            features.insert("unix".to_string());
        }
        #[cfg(target_os = "linux")]
        {
            features.insert("gnu-linux".to_string());
            features.insert("linux".to_string());
            features.insert("posix".to_string());
            features.insert("unix".to_string());
        }
        #[cfg(windows)]
        features.insert("windows".to_string());

        // Architecture
        #[cfg(target_arch = "x86_64")]
        features.insert("x86-64".to_string());
        #[cfg(target_arch = "aarch64")]
        features.insert("aarch64".to_string());

        // Endianness
        #[cfg(target_endian = "little")]
        features.insert("little-endian".to_string());
        #[cfg(target_endian = "big")]
        features.insert("big-endian".to_string());

        Self { features }
    }

    pub fn has_feature(&self, name: &str) -> bool {
        self.features.contains(name)
    }

    pub fn all_features(&self) -> Vec<String> {
        self.features.iter().cloned().collect()
    }
}
```

**Step 2: Add `(features)` primitive**

File: `patina-runtime/src/stdlib/scheme_base.rs`

```rust
// Add to scheme_base primitives
fn features(features_list: Vec<Value>) -> Value {
    // Return list of symbols
    Value::from_iter(
        FEATURE_REGISTRY.all_features()
            .into_iter()
            .map(|s| Value::symbol(&s))
    )
}
```

**Step 3: Feature Requirement Evaluator**

File: `patina-frontend/src/cond_expand.rs`

```rust
pub fn evaluate_feature_requirement(
    req: &Value,
    features: &FeatureRegistry,
    can_load_library: impl Fn(&[String]) -> bool,
) -> Result<bool, ParseError> {
    match req {
        Value::Symbol(name) => Ok(features.has_feature(name)),

        Value::Pair(_) => {
            let list = expect_list(req)?;
            if list.is_empty() {
                return Err(ParseError::InvalidSyntax("Empty feature requirement".into()));
            }

            match list[0].as_symbol() {
                Some("and") => {
                    for sub_req in &list[1..] {
                        if !evaluate_feature_requirement(sub_req, features, &can_load_library)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                Some("or") => {
                    for sub_req in &list[1..] {
                        if evaluate_feature_requirement(sub_req, features, &can_load_library)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                Some("not") => {
                    if list.len() != 2 {
                        return Err(ParseError::InvalidSyntax("not requires exactly one argument".into()));
                    }
                    Ok(!evaluate_feature_requirement(&list[1], features, &can_load_library)?)
                }
                Some("library") => {
                    if list.len() != 2 {
                        return Err(ParseError::InvalidSyntax("library requires exactly one argument".into()));
                    }
                    let lib_name = parse_library_name(&list[1])?;
                    Ok(can_load_library(&lib_name))
                }
                _ => Err(ParseError::InvalidSyntax(format!(
                    "Unknown feature requirement: {}", req
                ))),
            }
        }

        _ => Err(ParseError::InvalidSyntax(format!(
            "Invalid feature requirement: {}", req
        ))),
    }
}
```

**Step 4: Library cond-expand**

Expand `cond-expand` declarations during library parsing:

```rust
// In parse_declaration()
"cond-expand" => {
    let clauses = &list[1..];
    for clause in clauses {
        let clause_list = Self::expect_list(clause)?;
        if clause_list.is_empty() {
            continue;
        }

        let (requirement, declarations) = if clause_list[0].as_symbol() == Some("else") {
            (true, &clause_list[1..])
        } else {
            let matches = evaluate_feature_requirement(
                &clause_list[0],
                &FEATURES,
                |name| can_load_library(name),
            )?;
            (matches, &clause_list[1..])
        };

        if requirement {
            // Splice in the matching declarations
            for decl in declarations {
                Self::parse_declaration(decl, exports, imports, body_elements)?;
            }
            return Ok(());  // Stop after first match
        }
    }
    // No clause matched - R7RS says behavior is unspecified
    // We'll just continue (empty expansion)
    Ok(())
}
```

**Step 5: Expression cond-expand**

Two options:

**Option A: Implement as macro** (preferred for consistency)
```scheme
;; In base-extras.scm
(define-syntax cond-expand
  (syntax-rules (else and or not library)
    ;; ... macro rules
  ))
```
Problem: Macros can't query features at compile time easily.

**Option B: Implement as special form**
Add to `special_forms/cond_expand.rs` - can access FeatureRegistry directly.

Recommend **Option B** for simplicity - cond-expand needs access to runtime feature detection.

---

### Phase 4: include-library-declarations (Low Priority)

Rarely used in practice. Can defer until needed.

Difference from `include`:
- `include` splices expressions into library **body**
- `include-library-declarations` splices **declarations** (export, import, etc.)

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

### Phase 1: SRFI Names ✅ COMPLETE (2025-12-05)
- [x] `(srfi 1)` library name parses
- [x] `(srfi 69)` library name parses
- [x] Negative integers rejected
- [x] Mixed names work: `(srfi 1 lists)`

### Phase 2: Include ✅ COMPLETE (2025-12-05)
- [x] `(include "file.scm")` loads and splices
- [x] Multiple files: `(include "a.scm" "b.scm")`
- [x] Subdirectory paths: `(include "sub/file.scm")`
- [x] Order preserved with `begin`
- [x] Missing file produces clear error
- [x] Circular include detected

### Phase 3: cond-expand + features ✅ COMPLETE (2025-12-05)
- [x] `(features)` returns list with `r7rs`, `patina`
- [x] Platform features: `darwin`/`gnu-linux`/`windows`
- [x] Architecture features: `x86-64`/`aarch64`
- [x] `cond-expand` in library declaration
- [x] `cond-expand` as expression
- [x] Feature requirements: `and`, `or`, `not`
- [x] `(library <name>)` requirement (returns false - no library loader access)
- [x] `else` clause

### Phase 4: Include-ci ✅ COMPLETE (2025-12-08)
- [x] Case-insensitive reading via `Parser::new_case_insensitive()`
- [x] `Lexer::new_case_insensitive()` constructor
- [x] All identifiers folded to lowercase in included file

### Phase 5: Include-library-declarations ✅ COMPLETE (2025-12-08)
- [x] Declaration splicing from external files
- [x] Supports export, import, begin, include declarations
- [x] Recursive processing of nested includes
- [x] Multiple files in single declaration

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
