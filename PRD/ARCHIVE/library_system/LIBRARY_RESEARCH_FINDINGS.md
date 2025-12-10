# Library System Research Findings

**Date:** 2025-11-11
**Researchers:** Analysis of chibi-scheme and Gauche
**Purpose:** Inform Patina library system design

---

## Executive Summary

Research of two major Scheme implementations reveals different approaches to R7RS library support:

- **Chibi-Scheme:** Native R7RS design, libraries as first-class concept, most logic in C
- **Gauche:** R7RS compatibility layer over native module system, uses macro transformation

**Key Takeaway:** Both keep core mechanics in the host language (C) and use Scheme for organization. This validates our Rust-native approach.

---

## Chibi-Scheme Analysis

### Architecture

**Library File Format:** `.sld` (Scheme Library Definition)

**Example - `(scheme lazy)`:**
```scheme
(define-library (scheme lazy)
  (import (chibi))
  (export delay force delay-force make-promise promise?)
  (begin
    (define (make-promise x)
      (if (promise? x) x (delay x)))))
```

### Key Observations

1. **Internal Bootstrap Library:**
   - Everything imports from `(chibi)` - the internal core library
   - `(chibi)` contains all primitives (implemented in C)
   - R7RS libraries just re-export from `(chibi)` with appropriate names

2. **Heavy Import Usage:**
   ```scheme
   (define-library (scheme base)
     (import (rename (except (chibi) equal?)
                     (let-syntax let-syntax/splicing)
                     (letrec-syntax letrec-syntax/splicing))
             (rename (chibi equiv) (equiv? equal?))
             (only (chibi string) string-map string-for-each)
             ;; ... more imports
             (srfi 9) (srfi 11) (srfi 39))
     (export * + - ... / < <= = => > >= _ abs and append ...))
   ```

3. **Library Loading (C implementation):**
   - `sexp_load_module_file()` in `eval.c`
   - Searches for `.sld` files in library path
   - Parses and evaluates library definitions
   - Manages library registry in C

4. **File Organization:**
   ```
   lib/
   ├── chibi/           # Internal chibi primitives
   │   ├── ...
   └── scheme/          # R7RS standard libraries
       ├── base.sld
       ├── case-lambda.sld
       ├── char.sld
       └── ...
   ```

### Import Resolution

Chibi implements all import forms:
- `(import (scheme base))` - simple
- `(only (scheme base) + - *)` - selective
- `(except (scheme base) +)` - exclusion
- `(prefix (scheme base) base:)` - prefix
- `(rename (scheme base) (+ add))` - rename

All implemented in C with Scheme-level declarations.

---

## Gauche Analysis

### Architecture

**Approach:** Macro-based transformation from R7RS to Gauche's native module system

**Example - R7RS library macro:**
```scheme
(define-syntax define-library
  (er-macro-transformer
   (^[f r c]
     ;; ... macro logic
     (match f
       [(_ name . decls)
        `(define-module ,(library-name->module-name name)
           (extend)
           ,@(map transform-decl decls))]))))
```

### Key Observations

1. **Compatibility Layer:**
   - R7RS `define-library` → Gauche `define-module`
   - R7RS `import` → Gauche `import` (different syntax)
   - Macro transformation at parse time

2. **Module Name Mapping:**
   ```scheme
   ;; R7RS library name → Gauche module name
   (scheme base) → scheme.base
   (mylib utils) → mylib.utils
   ```

3. **Import Transformation:**
   ```scheme
   ;; R7RS import spec → Gauche import spec
   (only (scheme base) + - *) →
   (import scheme.base :only (+ - *))

   (rename (scheme base) (+ add)) →
   (import scheme.base :rename ((+ add)))
   ```

4. **Declaration Transformation:**
   Each library declaration gets transformed:
   - `(export ...)` → `(export ...)`  (direct)
   - `(import ...)` → `(r7rs-import ...)` → Gauche import
   - `(begin ...)` → `(begin ...)`  (direct)
   - `(include ...)` → `(include ...)` (direct)
   - `(cond-expand ...)` → expanded and transformed

5. **Implementation Strategy:**
   ```scheme
   (define-library (mylib)
     (import (scheme base))
     (export foo bar)
     (begin
       (define (foo x) (+ x 1))
       (define (bar x) (* x 2))))

   ;; Expands to:
   (define-module mylib
     (extend)
     (r7rs-import (scheme base))
     (export foo bar)
     (begin
       (define (foo x) (+ x 1))
       (define (bar x) (* x 2))))
   ```

### Benefits of Macro Approach

**Pros:**
- Reuses existing Gauche module system
- No need to reimplement module loading
- Easier to maintain (less C code)

**Cons:**
- Two module systems (Gauche + R7RS)
- Potential semantic mismatches
- Harder to optimize

---

## Comparison

| Aspect | Chibi | Gauche | Patina (Proposed) |
|--------|-------|--------|-------------------|
| **Library Definition** | Native `.sld` files | Macro → `define-module` | Native in Rust |
| **Core Primitives** | C (in `(chibi)`) | C/C++ (built-in) | Rust (existing) |
| **Library Loading** | C (`sexp_load_module_file`) | Scheme macros | Rust |
| **Import Resolution** | C | Scheme (at expand time) | Rust |
| **Module System** | Libraries only | Modules + R7RS compat | Libraries only |
| **Complexity** | Medium (C code) | Low (macro layer) | Medium (Rust) |
| **Performance** | Fast (native) | Fast (native) | Fast (native) |

---

## Key Insights for Patina

### 1. Keep Primitives in Host Language

**Both implementations keep primitives in C/C++, NOT Scheme.**

Chibi's approach:
```scheme
(define-library (scheme base)
  (import (chibi))  ; <-- All primitives here (C implementation)
  (export + - * / ...))
```

**Implication for Patina:**
- Keep our ~100 Rust primitives as-is
- Create internal `(patina primitives)` library
- Have `(scheme base)` import and re-export

### 2. Library System Can Be Simple

Gauche shows library system can be implemented via macros. This means:
- Parser needs to understand `define-library` syntax
- Resolution logic is straightforward (pattern matching)
- No need for complex runtime library loader

**Implication for Patina:**
- Could implement as special form + registry (our approach)
- Or as macro that expands to internal forms (Gauche's approach)
- We chose special form for better control and error messages

### 3. Import Resolution Patterns

Both use similar import resolution logic:

```
Simple:  (lib-name) → load lib, import all exports
Only:    (only lib a b) → load lib, import only a, b
Except:  (except lib a) → load lib, import all except a
Prefix:  (prefix lib p:) → load lib, import with prefix
Rename:  (rename lib ((old new) ...)) → load lib, import with renames
```

**Implication for Patina:**
- Standard pattern matching on ImportSpec
- Build binding list: Vec<(String, Value)>
- Add to environment

### 4. Library File Organization

Both use similar directory structure:

```
lib/
├── internal-library/    (chibi/, gauche/)
└── scheme/              (R7RS standard libs)
    ├── base.sld/.scm
    ├── char.sld/.scm
    └── ...
```

**Implication for Patina:**
```
lib/
├── patina/              (internal primitives)
│   └── primitives.sld
└── scheme/              (R7RS standard libs)
    ├── base.sld
    ├── char.sld
    └── ...
```

### 5. Circular Dependency Handling

Neither implementation explicitly showed circular dependency detection in the code we reviewed, but it's a known concern.

**Implication for Patina:**
- Track library load stack
- Error if same library appears twice in stack
- Clear error message with dependency chain

---

## Design Recommendations for Patina

Based on this research:

### 1. Architecture: Rust-Native with Scheme Definitions

**DO:**
- ✅ Implement library mechanics in Rust (loading, resolution, registry)
- ✅ Keep all primitives in Rust
- ✅ Use `.sld` files for library definitions
- ✅ Create internal `(patina primitives)` with Rust exports

**DON'T:**
- ❌ Don't reimplement primitives in Scheme (Chibi does this in C, not Scheme)
- ❌ Don't use macro-based library system (harder to debug)
- ❌ Don't create two module systems

### 2. Import Resolution: Pattern Matching

```rust
fn resolve_import(spec: &ImportSpec, registry: &LibraryRegistry)
    -> Result<Vec<(String, Value)>, LibraryError>
{
    match spec {
        ImportSpec::Simple(name) => load_all(name),
        ImportSpec::Only(name, idents) => load_only(name, idents),
        ImportSpec::Except(name, idents) => load_except(name, idents),
        ImportSpec::Prefix(name, prefix) => load_with_prefix(name, prefix),
        ImportSpec::Rename(name, renames) => load_with_renames(name, renames),
    }
}
```

### 3. Library File Format

**Option A: Chibi-style (our current design)**
```scheme
(define-library (mylib)
  (import (scheme base))
  (export foo)
  (begin (define (foo x) (+ x 1))))
```

**Option B: Could add Patina-specific forms**
```scheme
(define-library (patina primitives)
  ;; Special form: tells loader to export Rust primitives
  (export-rust-primitives + - * / cons car cdr ...))
```

Recommend: **Option A for R7RS libraries, Option B for internal**

### 4. Standard Library Organization

Follow both chibi and Gauche:

```
lib/patina/primitives.sld       # Internal: Rust primitives
lib/scheme/base.sld             # Imports patina primitives, exports R7RS
lib/scheme/char.sld             # Character operations
lib/scheme/file.sld             # File I/O
```

### 5. Error Messages

Learn from Gauche's helpful warnings:
```scheme
(when (equal? import-set '(gauche))
  (warn "(import (gauche)) does not import anything. ..."))
```

**For Patina:**
- Clear error when library not found (show search paths)
- Warn about common mistakes
- Show available exports on import error

---

## Implementation Strategy

Based on research, recommend this approach:

### Phase 1: Core Infrastructure (Rust)

```rust
// Library type
pub struct Library {
    name: Vec<String>,
    exports: HashMap<String, Value>,
    env: Rc<Environment>,
}

// Registry
pub struct LibraryRegistry {
    libraries: HashMap<Vec<String>, Library>,
    search_paths: Vec<PathBuf>,
}
```

### Phase 2: Parser (Rust + Scheme)

```rust
pub enum LibraryDeclaration {
    Import(ImportSpec),
    Export(Vec<ExportSpec>),
    Begin(Vec<Value>),
    Include(String),
}

pub fn parse_library_file(path: &Path)
    -> Result<(Vec<String>, Vec<LibraryDeclaration>), ParseError>;
```

### Phase 3: Resolution (Rust)

```rust
impl LibraryRegistry {
    pub fn load(&mut self, name: &[String])
        -> Result<&Library, LibraryError> {
        // 1. Check if already loaded
        // 2. Find .sld file in search paths
        // 3. Parse library declarations
        // 4. Resolve imports (recursive)
        // 5. Evaluate library body
        // 6. Build export map
        // 7. Register library
    }
}
```

### Phase 4: Standard Libraries (Scheme)

```scheme
;; lib/patina/primitives.sld
(define-library (patina primitives)
  (export-rust-primitives
    + - * / = < > <= >=
    cons car cdr list append
    ;; ... all our Rust primitives
    ))

;; lib/scheme/base.sld
(define-library (scheme base)
  (import (patina primitives))
  (export + - * / = < > ...)
  (include "base-helpers.scm"))
```

---

## Validation Against R7RS

### Required Features

R7RS Section 5.6 requires:

- [x] `define-library` syntax
- [x] `import` declarations with:
  - [x] Simple: `(lib-name)`
  - [x] `only`
  - [x] `except`
  - [x] `prefix`
  - [x] `rename`
- [x] `export` declarations with:
  - [x] Simple identifiers
  - [x] `rename`
- [x] `begin` for definitions
- [x] `include` for file inclusion
- [x] `include-ci` (case-insensitive)
- [x] `cond-expand` for conditional compilation

### Standard Libraries Required

- [x] `(scheme base)`
- [x] `(scheme case-lambda)`
- [x] `(scheme char)`
- [x] `(scheme complex)`
- [x] `(scheme cxr)`
- [x] `(scheme eval)`
- [x] `(scheme file)`
- [x] `(scheme inexact)`
- [x] `(scheme lazy)`
- [x] `(scheme load)`
- [x] `(scheme process-context)`
- [x] `(scheme read)`
- [x] `(scheme repl)`
- [x] `(scheme time)`
- [x] `(scheme write)`
- [x] `(scheme r5rs)`

All can be defined as `.sld` files importing from `(patina primitives)`.

---

## Open Questions Resolved

### Q1: Should we implement library system as macro or special form?

**Answer:** Special form (like chibi)
- Better error messages
- More control over loading
- Easier to debug
- Gauche uses macros because they already have a module system

### Q2: Should primitives be in Scheme or Rust?

**Answer:** Rust (like chibi's C, Gauche's C++)
- Both implementations keep primitives in host language
- Only library organization is in Scheme
- Performance and type safety

### Q3: How to handle circular dependencies?

**Answer:** Track load stack, error if cycle detected
- Both implementations must handle this
- Simple stack tracking during recursive load
- Clear error with dependency chain

### Q4: Library file extension?

**Answer:** `.sld` (Scheme Library Definition)
- Standard R7RS convention
- Used by chibi
- Clear distinction from regular `.scm` files

---

## Conclusion

Research of chibi-scheme and Gauche validates our Rust-native approach:

1. ✅ **Keep primitives in host language** (Rust, not Scheme)
2. ✅ **Library mechanics in host language** (Rust, not macros)
3. ✅ **Library definitions in Scheme** (`.sld` files)
4. ✅ **Follow R7RS library structure** (import/export/begin/include)
5. ✅ **Use standard file organization** (lib/patina/, lib/scheme/)

This approach combines:
- **Chibi's** native library support (but in Rust instead of C)
- **Gauche's** clarity of transformation logic (but at runtime in Rust)
- **R7RS** compliance for library syntax and standard libraries

**Next Steps:**
1. Implement Phase 1: Library infrastructure (Rust)
2. Implement Phase 2: Parser support
3. Implement Phase 3: Import resolution
4. Implement Phase 4: Standard library definitions
5. Test with chibi's r7rs-tests.scm

---

**Document Status:** Research Complete, Ready for Implementation
**Last Updated:** 2025-11-11
