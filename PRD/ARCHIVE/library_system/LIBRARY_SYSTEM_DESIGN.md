# Patina Library System Design

**Created:** 2025-11-11
**Status:** Design Phase
**Priority:** CRITICAL (blocks R7RS compliance)

---

## Executive Summary

This document outlines the design for Patina's R7RS library system based on research of chibi-scheme and practical Rust-based implementation choices. Unlike chibi which implements most functionality in C with thin Scheme wrappers, we'll take a **balanced approach**: library mechanics in Rust for robustness, but library *definitions* in Scheme for flexibility.

**Key Decision:** Library system will be **Rust-native** with Scheme-based library definitions. We will NOT reimplement everything in Scheme like chibi does.

---

## Research Findings

### Chibi-Scheme Architecture

**Key Observations from chibi:**

1. **Library Definition Format** (`.sld` files):
```scheme
(define-library (library name parts ...)
  (import ...)
  (export ...)
  (begin ...)
  (include "file.scm"))
```

2. **Standard Library Structure:**
   - `(scheme base)` - Core, imports from internal `(chibi)` library
   - Other libraries like `(scheme case-lambda)` just alias SRFIs
   - Heavy use of `import`, `export`, `rename`, `only`, `except`, `prefix`

3. **Implementation Strategy:**
   - Most primitives in C (`eval.c`, `sexp.c`)
   - Library loading in C (`sexp_load_module_file`)
   - `.sld` files define library boundaries
   - Uses internal `(chibi)` library with all primitives
   - R7RS libraries import/re-export from `(chibi)`

**Example - chibi's `(scheme base)`:**
```scheme
(define-library (scheme base)
  (import (rename (except (chibi) equal?)
                  (let-syntax let-syntax/splicing))
          (rename (chibi equiv) (equiv? equal?))
          (only (chibi string) string-map string-for-each)
          ;; ... more imports
          (srfi 9) (srfi 11) (srfi 39))
  (export * + - / < <= = ...))
```

**Takeaway:** Chibi keeps core in C but uses library system to organize APIs.

---

## Patina's Design Philosophy

### Core Principles

1. **Rust for Mechanics, Scheme for Organization**
   - Library loading, resolution, environment management: **Rust**
   - Library definitions (import/export specs): **Scheme (.sld files)**
   - Primitives stay in Rust (already ~100 primitives implemented)

2. **Practical Over Pure**
   - Not aiming to move everything to Scheme
   - Leverage Rust's type safety and performance
   - Keep primitives in Rust where it makes sense

3. **R7RS Compliance First**
   - Support all required library features
   - Match chibi's library resolution behavior
   - Enable standard library structure

4. **Future-Proof**
   - Support for bytecode VM (Phase 2)
   - Support for JIT (Phase 3)
   - Modular enough to add new backends

---

## Architecture Design

### 1. Library Value Type

Add a new `Value::Library` variant:

```rust
// crates/patina-runtime/src/value/mod.rs
pub enum Value {
    // ... existing variants
    Library(Library),
}

// crates/patina-runtime/src/library.rs
#[derive(Debug, Clone)]
pub struct Library {
    /// Library name: (scheme base) → vec!["scheme", "base"]
    pub name: Vec<String>,

    /// Exported bindings: name → value
    pub exports: HashMap<String, Value>,

    /// Library environment (for internal definitions)
    pub env: Rc<Environment>,

    /// Source file (for debugging)
    pub source: Option<PathBuf>,
}
```

### 2. Library Registry

Global registry for loaded libraries:

```rust
// crates/patina-runtime/src/library_registry.rs
pub struct LibraryRegistry {
    /// Loaded libraries: name → Library
    libraries: HashMap<Vec<String>, Library>,

    /// Search paths for .sld files
    search_paths: Vec<PathBuf>,
}

impl LibraryRegistry {
    pub fn new() -> Self { /* ... */ }

    /// Load library by name, searching paths
    pub fn load(&mut self, name: &[String]) -> Result<&Library, LibraryError>;

    /// Register a library
    pub fn register(&mut self, lib: Library);

    /// Check if library is loaded
    pub fn is_loaded(&self, name: &[String]) -> bool;

    /// Add search path for libraries
    pub fn add_search_path(&mut self, path: PathBuf);
}
```

### 3. Library Declaration Parser

Parse `define-library` forms:

```rust
// crates/patina-frontend/src/library/mod.rs
pub struct LibraryDeclaration {
    pub name: Vec<String>,
    pub imports: Vec<ImportSpec>,
    pub exports: Vec<ExportSpec>,
    pub body: Vec<Value>, // begin forms, includes, etc.
}

pub enum ImportSpec {
    Simple(Vec<String>),               // (import (scheme base))
    Only(Vec<String>, Vec<String>),    // (only (scheme base) + - *)
    Except(Vec<String>, Vec<String>),  // (except (scheme base) +)
    Prefix(Vec<String>, String),       // (prefix (scheme base) base:)
    Rename(Vec<String>, Vec<(String, String)>), // (rename (scheme base) (+ add))
}

pub enum ExportSpec {
    Simple(String),                    // foo
    Rename(String, String),            // (rename internal external)
}
```

### 4. Import Resolution

Resolve imports and populate environment:

```rust
// crates/patina-tree-walker/src/eval/library.rs
impl Evaluator {
    /// Process import declaration
    pub fn eval_import(&self, spec: &ImportSpec, registry: &LibraryRegistry)
        -> Result<Vec<(String, Value)>, EvalError>;

    /// Resolve import spec to (name, value) pairs
    fn resolve_import(&self, spec: &ImportSpec, registry: &LibraryRegistry)
        -> Result<Vec<(String, Value)>, EvalError> {
        match spec {
            ImportSpec::Simple(lib_name) => {
                let lib = registry.load(lib_name)?;
                Ok(lib.exports.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            }
            ImportSpec::Only(lib_name, names) => {
                let lib = registry.load(lib_name)?;
                Ok(names.iter()
                    .filter_map(|n| lib.exports.get(n).map(|v| (n.clone(), v.clone())))
                    .collect())
            }
            ImportSpec::Except(lib_name, names) => {
                let lib = registry.load(lib_name)?;
                Ok(lib.exports.iter()
                    .filter(|(k, _)| !names.contains(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect())
            }
            ImportSpec::Prefix(lib_name, prefix) => {
                let lib = registry.load(lib_name)?;
                Ok(lib.exports.iter()
                    .map(|(k, v)| (format!("{}{}", prefix, k), v.clone()))
                    .collect())
            }
            ImportSpec::Rename(lib_name, renames) => {
                let lib = registry.load(lib_name)?;
                let rename_map: HashMap<_, _> = renames.iter().cloned().collect();
                Ok(lib.exports.iter()
                    .map(|(k, v)| {
                        let new_name = rename_map.get(k).unwrap_or(k);
                        (new_name.clone(), v.clone())
                    })
                    .collect())
            }
        }
    }
}
```

### 5. Library Loading Process

**High-Level Flow:**

1. **Parse Library File** (`.sld`)
   ```
   Read file → Tokenize → Parse → Extract LibraryDeclaration
   ```

2. **Resolve Imports**
   ```
   For each import spec:
     - Load dependency library (recursive)
     - Resolve bindings per spec (only, except, rename, etc.)
     - Add to library environment
   ```

3. **Evaluate Body**
   ```
   Create library environment
   Add imported bindings
   Evaluate begin forms
   Process includes
   Execute definitions
   ```

4. **Build Export Map**
   ```
   For each export spec:
     - Lookup binding in library environment
     - Add to export map (with renames if needed)
   ```

5. **Register Library**
   ```
   Store in registry for future imports
   ```

---

## Implementation Plan

### Phase 1: Core Infrastructure (3-4 days)

**Deliverables:**
- [ ] `Library` value type and struct
- [ ] `LibraryRegistry` implementation
- [ ] Basic library search path logic
- [ ] Library file discovery (`.sld` extension)

**Files:**
- `crates/patina-runtime/src/library.rs` (new)
- `crates/patina-runtime/src/library_registry.rs` (new)
- `crates/patina-runtime/src/value/mod.rs` (add Library variant)

**Testing:**
- Registry load/register mechanics
- Search path resolution
- Duplicate library detection

### Phase 2: Parser Support (2-3 days)

**Deliverables:**
- [ ] Parse `define-library` forms
- [ ] Parse import specs (simple, only, except, prefix, rename)
- [ ] Parse export specs (simple, rename)
- [ ] Parse `include` and `begin` clauses

**Files:**
- `crates/patina-frontend/src/library/mod.rs` (new module)
- `crates/patina-frontend/src/library/parser.rs`
- `crates/patina-frontend/src/library/types.rs`

**Testing:**
- Parse valid library definitions
- Reject invalid syntax
- Handle all import/export forms

### Phase 3: Import Resolution (2-3 days)

**Deliverables:**
- [ ] Resolve simple imports
- [ ] Resolve `only`, `except`, `prefix`, `rename`
- [ ] Circular dependency detection
- [ ] Dependency ordering

**Files:**
- `crates/patina-tree-walker/src/eval/library.rs` (new)
- Integration with evaluator

**Testing:**
- Import resolution for various specs
- Circular dependency rejection
- Missing library errors

### Phase 4: Library Evaluation (2-3 days)

**Deliverables:**
- [ ] Create library environment
- [ ] Evaluate library body
- [ ] Process `include` directives
- [ ] Build export map
- [ ] Handle `cond-expand` (optional)

**Files:**
- `crates/patina-tree-walker/src/eval/library.rs` (expand)
- `crates/patina-tree-walker/src/eval/special_forms.rs` (add define-library)

**Testing:**
- Library with simple definitions
- Library with includes
- Export resolution

### Phase 5: Standard Libraries (3-4 days)

**Deliverables:**
- [ ] Create `lib/scheme/` directory structure
- [ ] Define `(scheme base)` - re-export our primitives
- [ ] Define other standard libraries
- [ ] Map existing primitives to exports

**Library Files:**
```
lib/scheme/
  ├── base.sld
  ├── case-lambda.sld
  ├── char.sld
  ├── complex.sld
  ├── cxr.sld
  ├── eval.sld
  ├── file.sld
  ├── inexact.sld
  ├── lazy.sld
  ├── load.sld
  ├── process-context.sld
  ├── read.sld
  ├── repl.sld
  ├── time.sld
  ├── write.sld
  └── r5rs.sld
```

**Approach:**
Create internal `(patina primitives)` library with all our Rust primitives, then have `(scheme base)` import and re-export them.

### Phase 6: REPL & Integration (2 days)

**Deliverables:**
- [ ] REPL support for `import`
- [ ] Top-level import outside library context
- [ ] Integration with existing tests
- [ ] Update documentation

**Files:**
- `crates/patina-repl/src/repl/mod.rs`
- Update existing test imports

**Testing:**
- REPL import statements
- Program with library imports
- All existing tests still pass

---

## Library File Format

### Standard Library Definition

**File:** `lib/scheme/base.sld`

```scheme
(define-library (scheme base)
  ;; Import from internal Patina primitives
  (import (patina primitives)
          (patina macros))

  ;; Export all R7RS (scheme base) procedures
  (export
    ;; Special forms (handled in evaluator)
    quote if define set! lambda begin
    define-syntax syntax-rules

    ;; Arithmetic
    + - * / < <= = > >=
    abs quotient remainder modulo
    floor ceiling truncate round
    sqrt square expt

    ;; ... all other exports
    )

  ;; Optional: include helper definitions
  (begin
    ;; Any Scheme-level helpers can go here
    ))
```

### Internal Primitives Library

**File:** `lib/patina/primitives.sld`

```scheme
(define-library (patina primitives)
  ;; This is a special marker - tells loader to use native Rust primitives
  (export-rust-primitives
    + - * / = < > <= >=
    cons car cdr list append reverse
    ;; ... all our Rust primitives
    ))
```

**Implementation Note:** The `export-rust-primitives` form is Patina-specific syntax that tells the library loader to populate exports from our primitive registry instead of evaluating Scheme code.

### User Library Example

**File:** `examples/mylib.sld`

```scheme
(define-library (mylib)
  (import (scheme base))
  (export square-list add-one)

  (begin
    (define (add-one x) (+ x 1))

    (define (square-list lst)
      (map (lambda (x) (* x x)) lst))))
```

**Usage:**

```scheme
(import (scheme base))
(import (mylib))

(square-list '(1 2 3 4))  ; => (1 4 9 16)
(add-one 41)              ; => 42
```

---

## File Organization

### New Directory Structure

```
patina/
├── lib/
│   ├── patina/
│   │   ├── primitives.sld        # Internal: all Rust primitives
│   │   └── macros.sld             # Internal: macro system
│   └── scheme/
│       ├── base.sld               # (scheme base)
│       ├── case-lambda.sld        # (scheme case-lambda)
│       ├── char.sld               # (scheme char)
│       ├── complex.sld            # (scheme complex)
│       ├── cxr.sld                # (scheme cxr)
│       ├── eval.sld               # (scheme eval)
│       ├── file.sld               # (scheme file)
│       ├── inexact.sld            # (scheme inexact)
│       ├── lazy.sld               # (scheme lazy)
│       ├── load.sld               # (scheme load)
│       ├── process-context.sld    # (scheme process-context)
│       ├── read.sld               # (scheme read)
│       ├── repl.sld               # (scheme repl)
│       ├── time.sld               # (scheme time)
│       ├── write.sld              # (scheme write)
│       └── r5rs.sld               # (scheme r5rs)
│
├── crates/
│   ├── patina-runtime/
│   │   └── src/
│   │       ├── library.rs              # NEW: Library type
│   │       ├── library_registry.rs     # NEW: Registry
│   │       └── value/mod.rs            # Add Library variant
│   │
│   ├── patina-frontend/
│   │   └── src/
│   │       └── library/                # NEW: Library parsing
│   │           ├── mod.rs
│   │           ├── parser.rs
│   │           └── types.rs
│   │
│   └── patina-tree-walker/
│       └── src/
│           └── eval/
│               ├── library.rs          # NEW: Library evaluation
│               ├── special_forms.rs    # Add define-library
│               └── mod.rs              # Integrate library loading
```

### Search Path Logic

**Default search paths (in order):**
1. `./lib/` (relative to current directory)
2. `$PATINA_HOME/lib/` (if env var set)
3. `/usr/local/share/patina/lib/` (Unix)
4. Executable directory `../lib/` (relative to binary)

**Library name to file mapping:**
- `(scheme base)` → search for `scheme/base.sld`
- `(mylib)` → search for `mylib.sld`
- `(mycompany mylib utils)` → search for `mycompany/mylib/utils.sld`

---

## Special Considerations

### 1. Bootstrap Problem

**Challenge:** `(scheme base)` needs to import primitives, but evaluator needs primitives to work.

**Solution:**
1. Evaluator has built-in primitive registry (Rust)
2. Create special internal library `(patina primitives)` that's pre-populated
3. `(scheme base)` imports from `(patina primitives)`
4. Users import from `(scheme base)`

### 2. Circular Dependencies

**Detection:**
- Track library load stack
- If library A is being loaded and tries to import library B which imports A → error

**Error Message:**
```
Circular library dependency detected:
  (mylib) imports (utils) imports (mylib)
```

### 3. Library vs REPL Environment

**REPL behavior:**
- Start with empty environment or `(scheme base)` pre-imported?
- R7RS says interaction-environment includes `(scheme base)` by default
- We should pre-import `(scheme base)` into REPL

**Implementation:**
```rust
impl Repl {
    pub fn new() -> Self {
        let registry = LibraryRegistry::new();
        let base = registry.load(&["scheme", "base"]).unwrap();
        let env = Environment::new();

        // Import (scheme base) into REPL environment
        for (name, value) in &base.exports {
            env.define(name.clone(), value.clone());
        }

        Repl { env, registry }
    }
}
```

### 4. Performance Considerations

**Library caching:**
- Once loaded, library stays in registry
- Exports are cloned when imported (cheap with Rc)
- No need to re-parse or re-evaluate

**File I/O:**
- Library files read once at load time
- Consider caching parsed LibraryDeclaration

---

## Testing Strategy

### Unit Tests

**Library Registry:**
```rust
#[test]
fn test_library_registration() {
    let mut registry = LibraryRegistry::new();
    let lib = Library::new(vec!["test".to_string()]);
    registry.register(lib);
    assert!(registry.is_loaded(&["test"]));
}

#[test]
fn test_circular_dependency() {
    // Test that circular imports are detected and rejected
}
```

**Import Resolution:**
```rust
#[test]
fn test_only_import() {
    // (import (only (scheme base) + -))
    // Should only import + and -, not other exports
}

#[test]
fn test_rename_import() {
    // (import (rename (scheme base) (+ add)))
    // Should import + as add
}
```

### Integration Tests

**Simple Library:**
```scheme
;; tests/libraries/simple.sld
(define-library (simple)
  (import (scheme base))
  (export double)
  (begin
    (define (double x) (* x 2))))
```

**Test usage:**
```rust
#[test]
fn test_simple_library() {
    let interp = Interpreter::new();
    let result = interp.eval_program(r#"
        (import (simple))
        (double 21)
    "#).unwrap();
    assert_eq!(result.to_string(), "42");
}
```

### R7RS Compliance Tests

Use chibi's `r7rs-tests.scm` which includes library tests.

---

## Migration Path

### Existing Code Migration

**Before (our current bootstrap.scm):**
```scheme
;; lib/bootstrap.scm
(define (not x) (if x #f #t))
(define (zero? x) (= x 0))
;; ... more definitions
```

**After (with library system):**
```scheme
;; lib/scheme/base.sld
(define-library (scheme base)
  (import (patina primitives))
  (export + - * / not zero? ...)
  (include "base-helpers.scm"))

;; lib/scheme/base-helpers.scm
(define (not x) (if x #f #t))
(define (zero? x) (= x 0))
```

### Transition Steps

1. **Phase 1:** Implement library system without breaking existing code
2. **Phase 2:** Create `(scheme base)` with all current functionality
3. **Phase 3:** Update REPL to auto-import `(scheme base)`
4. **Phase 4:** Update tests to use imports
5. **Phase 5:** Deprecate direct bootstrap.scm loading

---

## Error Handling

### Library Errors

```rust
#[derive(Debug)]
pub enum LibraryError {
    /// Library not found in search paths
    NotFound(Vec<String>),

    /// Circular dependency detected
    CircularDependency(Vec<Vec<String>>),

    /// Parse error in library file
    ParseError(String, usize), // file, line

    /// Import resolution failed
    ImportError(String), // detail

    /// Export not found
    ExportNotFound(Vec<String>, String), // lib, export name

    /// File I/O error
    IoError(std::io::Error),
}
```

### Error Messages

**Library not found:**
```
Error: Library (mylib) not found
Searched in:
  - ./lib/mylib.sld
  - /usr/local/share/patina/lib/mylib.sld
```

**Export not found:**
```
Error: (mylib) does not export 'foo'
Available exports: bar, baz, qux
```

**Circular dependency:**
```
Error: Circular library dependency:
  (mylib) imports
  (utils) imports
  (helpers) imports
  (mylib)
```

---

## Open Questions

### 1. Library Versioning

R7RS allows version numbers in library names: `(mylib (1 0))`.

**Decision:** Defer to Phase 2. For now, ignore version numbers.

### 2. Library Hygiene

Should macros defined in library A work when imported into library B?

**Decision:** Yes, use existing hygienic macro system. Library boundaries don't affect hygiene.

### 3. Compile-Time vs Runtime

When are libraries loaded? At parse time or evaluation time?

**Decision:** Load at import time (during evaluation). Allows REPL to dynamically import.

### 4. Library Caching

Should we cache parsed libraries on disk?

**Decision:** No, for Phase 1. Just in-memory caching. Consider disk cache in Phase 2 optimization.

---

## Success Criteria

**Library system is complete when:**

1. ✅ Can define libraries with `define-library`
2. ✅ Can import libraries with all import forms
3. ✅ Can export with rename
4. ✅ `(scheme base)` works and includes all our primitives
5. ✅ All 15 R7RS standard libraries defined
6. ✅ Existing tests pass with new library structure
7. ✅ REPL auto-imports `(scheme base)`
8. ✅ Can write and import user libraries
9. ✅ Circular dependencies detected and rejected
10. ✅ Clear error messages for library errors

---

## Timeline

**Total Estimated Effort:** 2 weeks (10 working days)

| Phase | Days | Cumulative |
|-------|------|------------|
| Phase 1: Infrastructure | 3-4 | Days 1-4 |
| Phase 2: Parser | 2-3 | Days 5-7 |
| Phase 3: Resolution | 2-3 | Days 8-10 |
| Phase 4: Evaluation | 2-3 | Days 11-13 |
| Phase 5: Standard Libs | 3-4 | Days 14-17 |
| Phase 6: Integration | 2 | Days 18-19 |
| **Buffer** | 1 | Day 20 |

**Target Completion:** 3 weeks from start (with buffer)

---

## Conclusion

This design provides a **practical, Rust-native** approach to R7RS library system that:

- ✅ Maintains our existing Rust primitive implementations
- ✅ Provides R7RS-compliant library organization
- ✅ Enables standard library structure
- ✅ Supports user-defined libraries
- ✅ Sets foundation for future backends (VM, JIT)

**Next Steps:**
1. Review and approve design
2. Begin Phase 1 implementation
3. Create feature branch: `feature/library-system`
4. Implement incrementally with tests

---

**Document Status:** Ready for Implementation
**Last Updated:** 2025-11-11
